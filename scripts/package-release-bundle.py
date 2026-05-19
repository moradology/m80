#!/usr/bin/env python3
"""Build a matched m80 release bundle from already-built inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib


BUNDLE_SCHEMA_VERSION = 1
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", required=True, help="GitHub release tag, e.g. v0.1.0")
    parser.add_argument("--target", default="linux-x86_64")
    parser.add_argument("--image-kind", default="minimal")
    parser.add_argument("--m80-bin", required=True, type=Path)
    parser.add_argument("--jailer-harden-bin", required=True, type=Path)
    parser.add_argument("--net-helper-bin", required=True, type=Path)
    parser.add_argument("--kernel", required=True, type=Path)
    parser.add_argument("--rootfs", required=True, type=Path)
    parser.add_argument("--rootfs-manifest", required=True, type=Path)
    parser.add_argument("--build-receipt", required=True, type=Path)
    parser.add_argument("--guestd", required=True, type=Path)
    parser.add_argument("--install-sh", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--repo-root", default=Path.cwd(), type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    out_dir = args.out_dir.resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    workspace_version = workspace_package_version(repo_root)
    expected_tag = f"v{workspace_version}"
    if args.release_tag != expected_tag:
        raise SystemExit(
            f"release tag mismatch: expected {expected_tag} from workspace version "
            f"{workspace_version}, got {args.release_tag}"
        )

    version = m80_version(args.m80_bin)
    require(version.get("binary_version") == args.release_tag, "m80 binary_version != release tag")
    require(version.get("release_tag") == args.release_tag, "m80 release_tag != release tag")
    require(version.get("package_version") == workspace_version, "m80 package_version != workspace version")
    require(version.get("version_status") == "release", "m80 version_status must be release")

    guest_manifest = read_json(args.rootfs_manifest)
    manifest_schema = guest_manifest.get("schema_version")
    expected_fc = guest_manifest.get("expected_firecracker_version")
    require(isinstance(manifest_schema, int), "guest manifest missing integer schema_version")
    require(isinstance(expected_fc, str) and expected_fc, "guest manifest missing expected_firecracker_version")

    with tempfile.TemporaryDirectory(prefix="m80-release-bundle-") as tmp:
        bundle_root = Path(tmp) / "bundle"
        file_map = {
            "bin/m80": args.m80_bin,
            "bin/m80-jailer-harden": args.jailer_harden_bin,
            "bin/m80-net-helper": args.net_helper_bin,
            "artifacts/vmlinux": args.kernel,
            "artifacts/output.ext4": args.rootfs,
            "artifacts/output.ext4.manifest.json": args.rootfs_manifest,
            "artifacts/output.ext4.build-receipt.json": args.build_receipt,
            "artifacts/m80-guestd": args.guestd,
            "install.sh": args.install_sh,
        }
        for rel, src in file_map.items():
            copy_file(src, bundle_root / rel)

        set_executable(bundle_root / "bin/m80")
        set_executable(bundle_root / "bin/m80-jailer-harden")
        set_executable(bundle_root / "bin/m80-net-helper")
        set_executable(bundle_root / "install.sh")

        metadata = bundle_metadata(
            args=args,
            version=version,
            manifest_schema=manifest_schema,
            expected_fc=expected_fc,
            files=file_hashes(bundle_root),
        )
        metadata_path = bundle_root / "bundle.json"
        metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")

        sums = file_hashes(bundle_root)
        write_sha256s(bundle_root / "SHA256SUMS", sums)

        tarball = out_dir / BUNDLE_NAME
        with tarfile.open(tarball, "w:gz") as tar:
            for path in sorted(bundle_root.rglob("*")):
                if path.is_file():
                    tar.add(path, arcname=path.relative_to(bundle_root))
        (out_dir / f"{BUNDLE_NAME}.sha256").write_text(
            f"{sha256(tarball)}  {BUNDLE_NAME}\n"
        )
        shutil.copy2(args.install_sh, out_dir / "install.sh")
        print(tarball)
    return 0


def workspace_package_version(repo_root: Path) -> str:
    with (repo_root / "Cargo.toml").open("rb") as f:
        cargo = tomllib.load(f)
    return cargo["workspace"]["package"]["version"]


def m80_version(binary: Path) -> dict:
    output = subprocess.run(
        [str(binary), "--json", "version"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if output.returncode != 0:
        raise SystemExit(f"{binary} --json version failed: {output.stderr.strip()}")
    payload = json.loads(output.stdout)
    return payload["data"]


def read_json(path: Path) -> dict:
    with path.open() as f:
        return json.load(f)


def copy_file(src: Path, dest: Path) -> None:
    require(src.is_file(), f"missing required input: {src}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dest)


def set_executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | 0o755)


def file_hashes(root: Path) -> list[dict]:
    rows = []
    for path in sorted(root.rglob("*")):
        if path.is_file():
            rel = path.relative_to(root).as_posix()
            if rel == "SHA256SUMS":
                continue
            rows.append({"path": rel, "sha256": sha256(path), "size_bytes": path.stat().st_size})
    return rows


def bundle_metadata(
    *,
    args: argparse.Namespace,
    version: dict,
    manifest_schema: int,
    expected_fc: str,
    files: list[dict],
) -> dict:
    return {
        "schema_version": BUNDLE_SCHEMA_VERSION,
        "release_tag": args.release_tag,
        "m80_version": version["binary_version"],
        "package_version": version["package_version"],
        "target": args.target,
        "image_kind": args.image_kind,
        "guest_protocol_version": version["protocol_version"],
        "manifest_schema_version": manifest_schema,
        "expected_firecracker_version": expected_fc,
        "files": files,
    }


def write_sha256s(path: Path, rows: list[dict]) -> None:
    lines = [f"{row['sha256']}  {row['path']}\n" for row in rows]
    path.write_text("".join(lines))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
