#!/usr/bin/env python3
"""Verify the m80 release bundle contract."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import tarfile
import tomllib


BUNDLE_SCHEMA_VERSION = 1
PAYLOAD_PATHS = {
    "bin/m80",
    "bin/m80-jailer-harden",
    "bin/m80-net-helper",
    "artifacts/vmlinux",
    "artifacts/output.ext4",
    "artifacts/output.ext4.manifest.json",
    "artifacts/output.ext4.build-receipt.json",
    "artifacts/m80-guestd",
    "install.sh",
}
REQUIRED_PATHS = PAYLOAD_PATHS | {"bundle.json", "SHA256SUMS"}
FORBIDDEN_PATHS = {"artifacts/host-binaries.manifest.json"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--target", default="linux-x86_64")
    parser.add_argument("--image-kind", default="minimal")
    parser.add_argument("--repo-root", default=Path.cwd(), type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    workspace_version = workspace_package_version(args.repo_root)
    expected_tag = f"v{workspace_version}"
    require(args.release_tag == expected_tag, f"release tag mismatch: expected {expected_tag}, got {args.release_tag}")

    with tarfile.open(args.bundle, "r:gz") as tar:
        files = read_regular_files(tar)

    paths = set(files)
    missing = sorted(REQUIRED_PATHS - paths)
    require(not missing, f"bundle missing required paths: {', '.join(missing)}")
    forbidden = sorted(FORBIDDEN_PATHS & paths)
    require(not forbidden, f"bundle contains install-time-only paths: {', '.join(forbidden)}")

    metadata = json.loads(files["bundle.json"].decode("utf-8"))
    require(metadata.get("schema_version") == BUNDLE_SCHEMA_VERSION, "unsupported bundle schema_version")
    require(metadata.get("release_tag") == args.release_tag, "bundle release_tag mismatch")
    require(metadata.get("m80_version") == args.release_tag, "bundle m80_version mismatch")
    require(metadata.get("package_version") == workspace_version, "bundle package_version mismatch")
    require(metadata.get("target") == args.target, "bundle target mismatch")
    require(metadata.get("image_kind") == args.image_kind, "bundle image_kind mismatch")
    require(isinstance(metadata.get("manifest_schema_version"), int), "bundle manifest_schema_version missing")
    require(isinstance(metadata.get("guest_protocol_version"), int), "bundle guest_protocol_version missing")
    require(metadata.get("expected_firecracker_version"), "bundle expected_firecracker_version missing")

    metadata_files = metadata_file_map(metadata)
    require(set(metadata_files) == PAYLOAD_PATHS, "bundle metadata file set mismatch")
    for path, expected in metadata_files.items():
        actual = sha256_bytes(files[path])
        require(actual == expected, f"bundle metadata hash mismatch for {path}")

    sums = parse_sha256s(files["SHA256SUMS"].decode("utf-8"))
    require(set(sums) == PAYLOAD_PATHS | {"bundle.json"}, "SHA256SUMS file set mismatch")
    for path, expected in sums.items():
        actual = sha256_bytes(files[path])
        require(actual == expected, f"SHA256SUMS hash mismatch for {path}")

    print(f"verified {args.bundle}")
    return 0


def read_regular_files(tar: tarfile.TarFile) -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for member in tar.getmembers():
        name = member.name
        require(not name.startswith("/"), f"bundle path must be relative: {name}")
        require(".." not in Path(name).parts, f"bundle path must not escape root: {name}")
        if not member.isfile():
            continue
        require(name not in files, f"bundle duplicate path: {name}")
        extracted = tar.extractfile(member)
        require(extracted is not None, f"bundle member unreadable: {name}")
        files[name] = extracted.read()
    return files


def metadata_file_map(metadata: dict) -> dict[str, str]:
    rows = metadata.get("files")
    require(isinstance(rows, list), "bundle files must be a list")
    result = {}
    for row in rows:
        require(isinstance(row, dict), "bundle file entry must be an object")
        path = row.get("path")
        digest = row.get("sha256")
        require(isinstance(path, str), "bundle file entry missing path")
        require(isinstance(digest, str), f"bundle file entry missing sha256 for {path}")
        require(path not in result, f"bundle metadata duplicate path: {path}")
        result[path] = digest
    return result


def parse_sha256s(text: str) -> dict[str, str]:
    result = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        digest, path = line.split(maxsplit=1)
        path = path.strip()
        require(path not in result, f"SHA256SUMS duplicate path: {path}")
        result[path] = digest
    return result


def workspace_package_version(repo_root: Path) -> str:
    with (repo_root / "Cargo.toml").open("rb") as f:
        cargo = tomllib.load(f)
    return cargo["workspace"]["package"]["version"]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
