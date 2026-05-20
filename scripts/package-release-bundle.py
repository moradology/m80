#!/usr/bin/env python3
"""Build a matched m80 release bundle from already-built inputs."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

from release_url_contract import public_release_root, release_asset_url, release_repository


BUNDLE_SCHEMA_VERSION = 1
ASSET_INDEX_SCHEMA_VERSION = 1
BOOTSTRAP_SELECTOR_SCHEMA_VERSION = 1
BUILD_MANIFEST_SCHEMA_VERSION = 1
SUPPORTED_TARGET = "linux-x86_64"
SUPPORTED_IMAGE_KIND = "minimal"
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
ASSET_INDEX_NAME = "m80-release-assets.json"
BOOTSTRAP_SELECTOR_NAME = "m80-bootstrap-selector.tsv"
BUILD_MANIFEST_NAME = "m80-release-build.json"
INSTALL_NAME = "install.sh"
INTEGRITY_NAME = "m80-release-integrity.json"
INTEGRITY_ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"
GUESTD_VERSION_RE = re.compile(r"^m80-guestd (?P<package_version>\S+) \(proto v(?P<protocol_version>\d+)\)\s*$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
STABLE_RELEASE_TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+$")
INSTALL_TEMPLATE_TOKENS = {
    "@M80_RELEASE_TAG@",
    "@M80_PUBLIC_RELEASE_OWNER@",
    "@M80_PUBLIC_RELEASE_REPO@",
}
REQUIRED_MINIMAL_ARTIFACTS = {"kernel_image", "output_rootfs_image", "daemon_binary_path"}
INTEGRITY_SUBJECT_KINDS = [
    (BUNDLE_NAME, "release-bundle"),
    (f"{BUNDLE_NAME}.sha256", "checksum-sidecar"),
    (INSTALL_NAME, "installer"),
    (f"{INSTALL_NAME}.sha256", "checksum-sidecar"),
    (METADATA_NAME, "bundle-metadata"),
    (f"{METADATA_NAME}.sha256", "checksum-sidecar"),
    (ASSET_INDEX_NAME, "asset-index"),
    (f"{ASSET_INDEX_NAME}.sha256", "checksum-sidecar"),
    (BOOTSTRAP_SELECTOR_NAME, "bootstrap-selector"),
    (f"{BOOTSTRAP_SELECTOR_NAME}.sha256", "checksum-sidecar"),
    (BUILD_MANIFEST_NAME, "build-manifest"),
    (f"{BUILD_MANIFEST_NAME}.sha256", "checksum-sidecar"),
    ("SHA256SUMS", "checksum-manifest"),
]
BOOTSTRAP_SELECTOR_COLUMNS = [
    "os",
    "arch",
    "image_kind",
    "bundle_name",
    "bundle_url",
    "bundle_sha256",
    "size_bytes",
    "metadata_name",
    "metadata_sha256",
    "checksum_name",
    "signature_name",
    "attestation_name",
    "m80_version",
]
SELECTOR_VALUE_RE = re.compile(r"^[A-Za-z0-9._:/+-]+$")
APT_PACKAGE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9+.-]*$")
OCI_SHA256_DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
FILE_MODES = {
    "bin/m80": 0o755,
    "bin/m80-jailer-harden": 0o755,
    "bin/m80-net-helper": 0o755,
    "artifacts/vmlinux": 0o644,
    "artifacts/output.ext4": 0o644,
    "artifacts/output.ext4.manifest.json": 0o644,
    "artifacts/output.ext4.build-receipt.json": 0o644,
    "artifacts/m80-guestd": 0o644,
    "install.sh": 0o755,
    "bundle.json": 0o644,
    "SHA256SUMS": 0o644,
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", required=True, help="GitHub release tag, e.g. v0.1.0")
    parser.add_argument("--commit-sha", required=True, help="40-character source commit SHA for release proof")
    parser.add_argument("--rust-toolchain", required=True, help="Rust toolchain used to build release binaries")
    parser.add_argument("--target-triple", action="append", required=True, help="Rust target triple built in this release")
    parser.add_argument("--builder-identity", required=True, help="Builder identity, e.g. GitHub Actions run URL")
    parser.add_argument("--builder-os-image", required=True, help="OS image used by the release builder")
    parser.add_argument(
        "--apt-package-version",
        action="append",
        default=[],
        metavar="PACKAGE=VERSION",
        help="Relevant apt package version installed in the release builder",
    )
    parser.add_argument("--container-digest", help="Container image digest if the builder is containerized")
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
    target_os, target_arch = supported_target_parts(args.target)
    require(COMMIT_RE.match(args.commit_sha) is not None, "commit sha must be a 40-character lowercase hex digest")
    require(args.rust_toolchain.strip(), "rust toolchain must not be empty")
    target_triples = target_triple_list(args.target_triple)
    apt_packages = apt_package_versions(args.apt_package_version)
    container_digest = container_image_digest(args.container_digest)
    require(args.builder_identity.strip(), "builder identity must not be empty")
    require(args.builder_os_image.strip(), "builder os image must not be empty")
    require(
        bool(apt_packages) or container_digest is not None,
        "release build manifest must record apt package versions or a container digest",
    )
    require(
        args.image_kind == SUPPORTED_IMAGE_KIND,
        f"unsupported image kind: expected {SUPPORTED_IMAGE_KIND}, got {args.image_kind}",
    )
    require(
        STABLE_RELEASE_TAG_RE.fullmatch(args.release_tag) is not None,
        f"stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: {args.release_tag}",
    )

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
    require(isinstance(version.get("protocol_version"), int), "m80 version missing protocol_version")
    require(isinstance(version.get("manifest_schema_version"), int), "m80 version missing manifest_schema_version")
    require(
        isinstance(version.get("build_receipt_schema_version"), int),
        "m80 version missing build_receipt_schema_version",
    )
    require(
        isinstance(version.get("install_provenance_schema_version"), int),
        "m80 version missing install_provenance_schema_version",
    )

    guestd = guestd_version(args.guestd)
    require(guestd["package_version"] == workspace_version, "m80-guestd package_version != workspace version")
    require(
        guestd["protocol_version"] == version["protocol_version"],
        "m80-guestd protocol_version != m80 protocol_version",
    )

    guest_manifest = read_json(args.rootfs_manifest)
    receipt = read_json(args.build_receipt)
    compatibility = verify_compatibility_tuple(
        args=args,
        version=version,
        guestd=guestd,
        guest_manifest=guest_manifest,
        receipt=receipt,
    )

    with tempfile.TemporaryDirectory(prefix="m80-release-bundle-") as tmp:
        bundle_root = Path(tmp) / "bundle"
        rendered_install = Path(tmp) / INSTALL_NAME
        render_install_script(
            args.install_sh,
            rendered_install,
            release_tag=args.release_tag,
        )
        file_map = {
            "bin/m80": args.m80_bin,
            "bin/m80-jailer-harden": args.jailer_harden_bin,
            "bin/m80-net-helper": args.net_helper_bin,
            "artifacts/vmlinux": args.kernel,
            "artifacts/output.ext4": args.rootfs,
            "artifacts/output.ext4.manifest.json": args.rootfs_manifest,
            "artifacts/output.ext4.build-receipt.json": args.build_receipt,
            "artifacts/m80-guestd": args.guestd,
            "install.sh": rendered_install,
        }
        for rel, src in file_map.items():
            copy_file(src, bundle_root / rel, FILE_MODES[rel])

        metadata = bundle_metadata(
            args=args,
            version=version,
            guestd=guestd,
            compatibility=compatibility,
            target_os=target_os,
            target_arch=target_arch,
            files=file_hashes(bundle_root),
        )
        metadata_path = bundle_root / "bundle.json"
        metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
        metadata_path.chmod(FILE_MODES["bundle.json"])

        sums = file_hashes(bundle_root)
        write_sha256s(bundle_root / "SHA256SUMS", sums)
        (bundle_root / "SHA256SUMS").chmod(FILE_MODES["SHA256SUMS"])

        tarball = out_dir / BUNDLE_NAME
        write_deterministic_tar_gz(bundle_root, tarball)
        tarball.chmod(0o644)
        write_sha256_sidecar(out_dir / f"{BUNDLE_NAME}.sha256", tarball, BUNDLE_NAME)
        install_asset = out_dir / INSTALL_NAME
        copy_file(rendered_install, install_asset, 0o755)
        write_sha256_sidecar(out_dir / f"{INSTALL_NAME}.sha256", install_asset, INSTALL_NAME)
        metadata_asset = out_dir / METADATA_NAME
        shutil.copy2(metadata_path, metadata_asset)
        metadata_asset.chmod(0o644)
        write_sha256_sidecar(out_dir / f"{METADATA_NAME}.sha256", metadata_asset, METADATA_NAME)
        build_manifest_path = out_dir / BUILD_MANIFEST_NAME
        write_json(
            build_manifest_path,
            release_build_manifest(
                args=args,
                package_version=workspace_version,
                metadata_asset=metadata_asset,
                target_triples=target_triples,
                apt_packages=apt_packages,
                container_digest=container_digest,
                repo_root=repo_root,
            ),
        )
        build_manifest_path.chmod(0o644)
        write_sha256_sidecar(
            out_dir / f"{BUILD_MANIFEST_NAME}.sha256",
            build_manifest_path,
            BUILD_MANIFEST_NAME,
        )
        asset_index_path = out_dir / ASSET_INDEX_NAME
        asset_index = release_asset_index(
            args=args,
            version=version,
            compatibility=compatibility,
            target_os=target_os,
            target_arch=target_arch,
            tarball=tarball,
            metadata_asset=metadata_asset,
        )
        write_json(
            asset_index_path,
            asset_index,
        )
        asset_index_path.chmod(0o644)
        write_sha256_sidecar(out_dir / f"{ASSET_INDEX_NAME}.sha256", asset_index_path, ASSET_INDEX_NAME)
        bootstrap_selector_path = out_dir / BOOTSTRAP_SELECTOR_NAME
        write_bootstrap_selector(bootstrap_selector_path, asset_index)
        bootstrap_selector_path.chmod(0o644)
        write_sha256_sidecar(
            out_dir / f"{BOOTSTRAP_SELECTOR_NAME}.sha256",
            bootstrap_selector_path,
            BOOTSTRAP_SELECTOR_NAME,
        )
        write_public_sha256s(
            out_dir / "SHA256SUMS",
            [
                (BUNDLE_NAME, tarball),
                (INSTALL_NAME, install_asset),
                (METADATA_NAME, metadata_asset),
                (ASSET_INDEX_NAME, asset_index_path),
                (BOOTSTRAP_SELECTOR_NAME, bootstrap_selector_path),
                (BUILD_MANIFEST_NAME, build_manifest_path),
            ],
        )
        integrity_path = out_dir / INTEGRITY_NAME
        write_json(
            integrity_path,
            release_integrity_material(
                args=args,
                package_version=workspace_version,
                metadata_asset=metadata_asset,
                dist_dir=out_dir,
            ),
        )
        integrity_path.chmod(0o644)
        print(tarball)
    return 0


def workspace_package_version(repo_root: Path) -> str:
    with (repo_root / "Cargo.toml").open("rb") as f:
        cargo = tomllib.load(f)
    return cargo["workspace"]["package"]["version"]


def m80_version(binary: Path) -> dict:
    require(binary.is_file(), f"missing required input: {binary}")
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


def guestd_version(binary: Path) -> dict:
    require(binary.is_file(), f"missing required input: {binary}")
    output = subprocess.run(
        [str(binary), "--version"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if output.returncode != 0:
        raise SystemExit(f"{binary} --version failed: {output.stderr.strip()}")
    match = GUESTD_VERSION_RE.match(output.stdout.strip())
    if not match:
        raise SystemExit(f"m80-guestd --version output was not parseable: {output.stdout.strip()}")
    return {
        "package_version": match.group("package_version"),
        "protocol_version": int(match.group("protocol_version")),
    }


def read_json(path: Path) -> dict:
    with path.open() as f:
        return json.load(f)


def supported_target_parts(target: str) -> tuple[str, str]:
    require(target == SUPPORTED_TARGET, f"unsupported target: expected {SUPPORTED_TARGET}, got {target}")
    return ("linux", "x86_64")


def verify_compatibility_tuple(
    *,
    args: argparse.Namespace,
    version: dict,
    guestd: dict,
    guest_manifest: dict,
    receipt: dict,
) -> dict:
    manifest_schema = require_int(guest_manifest, "schema_version", "guest manifest")
    require(
        manifest_schema == version["manifest_schema_version"],
        f"guest manifest schema_version mismatch: expected {version['manifest_schema_version']}, got {manifest_schema}",
    )
    expected_fc = require_str(guest_manifest, "expected_firecracker_version", "guest manifest")
    manifest_image_kind = require_str(guest_manifest, "image_kind", "guest manifest")
    require(
        manifest_image_kind == args.image_kind,
        f"guest manifest image_kind mismatch: expected {args.image_kind}, got {manifest_image_kind}",
    )
    require(
        guest_manifest.get("source_rootfs_image") is None and guest_manifest.get("source_rootfs_sha256") is None,
        "minimal guest manifest must not record source rootfs artifacts",
    )

    receipt_schema = require_int(receipt, "schema_version", "build receipt")
    require(
        receipt_schema == version["build_receipt_schema_version"],
        f"build receipt schema_version mismatch: expected {version['build_receipt_schema_version']}, got {receipt_schema}",
    )
    require(
        require_str(receipt, "manifest_sha256", "build receipt") == sha256(args.rootfs_manifest),
        "build receipt manifest_sha256 mismatch",
    )
    receipt_manifest_path = Path(require_str(receipt, "manifest_path", "build receipt"))
    require(
        receipt_manifest_path.resolve(strict=False) == args.rootfs_manifest.resolve(strict=False),
        f"build receipt manifest_path mismatch: expected {args.rootfs_manifest}, got {receipt_manifest_path}",
    )

    artifact_rows = receipt_artifact_map(receipt)
    require(
        set(artifact_rows) == REQUIRED_MINIMAL_ARTIFACTS,
        "build receipt artifact set mismatch",
    )
    verify_manifest_artifact(
        args.kernel,
        "kernel_image",
        "kernel_image_sha256",
        guest_manifest,
        artifact_rows,
    )
    verify_manifest_artifact(
        args.rootfs,
        "output_rootfs_image",
        "output_rootfs_sha256",
        guest_manifest,
        artifact_rows,
    )
    verify_manifest_artifact(
        args.guestd,
        "daemon_binary_path",
        "daemon_binary_sha256",
        guest_manifest,
        artifact_rows,
    )

    return {
        "manifest_schema": manifest_schema,
        "build_receipt_schema": receipt_schema,
        "build_receipt_manifest_path": str(receipt_manifest_path),
        "install_provenance_schema": version["install_provenance_schema_version"],
        "expected_firecracker_version": expected_fc,
        "m80_protocol_version": version["protocol_version"],
        "guest_protocol_version": guestd["protocol_version"],
    }


def verify_manifest_artifact(
    input_path: Path,
    manifest_path_field: str,
    manifest_sha_field: str,
    guest_manifest: dict,
    artifact_rows: dict[str, dict],
) -> None:
    manifest_path = require_str(guest_manifest, manifest_path_field, "guest manifest")
    manifest_sha = require_str(guest_manifest, manifest_sha_field, "guest manifest")
    require(manifest_sha == sha256(input_path), f"guest manifest {manifest_sha_field} mismatch")
    row = artifact_rows[manifest_path_field]
    require(row["path"] == manifest_path, f"build receipt {manifest_path_field} path mismatch")
    require(row["sha256"] == manifest_sha, f"build receipt {manifest_path_field} sha256 mismatch")


def receipt_artifact_map(receipt: dict) -> dict[str, dict]:
    rows = receipt.get("artifacts")
    require(isinstance(rows, list), "build receipt artifacts must be a list")
    result = {}
    for row in rows:
        require(isinstance(row, dict), "build receipt artifact entry must be an object")
        kind = require_str(row, "kind", "build receipt artifact")
        path = require_str(row, "path", "build receipt artifact")
        digest = require_str(row, "sha256", "build receipt artifact")
        require(kind not in result, f"build receipt duplicate artifact: {kind}")
        result[kind] = {"path": path, "sha256": digest}
    return result


def require_int(obj: dict, key: str, label: str) -> int:
    value = obj.get(key)
    require(isinstance(value, int), f"{label} missing integer {key}")
    return value


def require_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} missing {key}")
    return value


def target_triple_list(raw_triples: list[str]) -> list[str]:
    triples: list[str] = []
    seen: set[str] = set()
    for raw in raw_triples:
        triple = raw.strip()
        require(triple, "target triple must not be empty")
        require(
            not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in triple),
            f"target triple must not contain whitespace or control characters: {triple!r}",
        )
        if triple in seen:
            continue
        seen.add(triple)
        triples.append(triple)
    require(triples, "at least one target triple must be recorded")
    require(
        "x86_64-unknown-linux-musl" in seen,
        "linux-x86_64 release builds must record x86_64-unknown-linux-musl",
    )
    return triples


def apt_package_versions(raw_packages: list[str]) -> list[dict[str, str]]:
    packages = []
    seen: set[str] = set()
    for raw in raw_packages:
        require("=" in raw, f"apt package version must be PACKAGE=VERSION: {raw}")
        name, version = raw.split("=", 1)
        require(APT_PACKAGE_NAME_RE.fullmatch(name) is not None, f"apt package name invalid: {name}")
        require(version, f"apt package version missing for {name}")
        require(
            not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in version),
            f"apt package version must not contain whitespace or control characters: {name}",
        )
        require(name not in seen, f"duplicate apt package version: {name}")
        seen.add(name)
        packages.append({"name": name, "version": version})
    return sorted(packages, key=lambda row: row["name"])


def container_image_digest(value: str | None) -> str | None:
    if value is None:
        return None
    require(
        OCI_SHA256_DIGEST_RE.fullmatch(value) is not None,
        "container digest must be sha256:<64 lowercase hex>",
    )
    return value


def copy_file(src: Path, dest: Path, mode: int) -> None:
    require(src.is_file(), f"missing required input: {src}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dest)
    dest.chmod(mode)


def render_install_script(
    template: Path,
    dest: Path,
    *,
    release_tag: str,
) -> None:
    require(template.is_file(), f"missing required input: {template}")
    text = template.read_text()
    validate_install_template(template, text)
    release_root = public_release_root()
    rendered = (
        text.replace("@M80_RELEASE_TAG@", release_tag)
        .replace("@M80_PUBLIC_RELEASE_OWNER@", release_root.owner)
        .replace("@M80_PUBLIC_RELEASE_REPO@", release_root.repo)
    )
    require(
        not any(token in rendered for token in INSTALL_TEMPLATE_TOKENS),
        "install.sh template placeholders were not fully rendered",
    )
    dest.write_text(rendered)
    dest.chmod(FILE_MODES["install.sh"])


def validate_install_template(template: Path, text: str) -> None:
    legacy_needles = [
        "quickstart.sh",
        "m80 quickstart",
        "--artifact-url",
        "@M80_BUNDLE_URL@",
        "@M80_BUNDLE_NAME@",
        "M80_BUNDLE_URL=",
        "M80_BUNDLE_NAME=",
        BUNDLE_NAME,
        "m80-linux-x86_64-minimal-artifacts.tar.gz",
    ]
    found = [needle for needle in legacy_needles if needle in text]
    require(
        not found,
        f"install.sh must use selector-driven m80 install, not legacy quickstart or hardcoded bundle flow: {template}",
    )
    missing = sorted(token for token in INSTALL_TEMPLATE_TOKENS if token not in text)
    require(
        not missing,
        f"install.sh template missing required placeholder(s): {', '.join(missing)}",
    )
    require(
        "bin/m80" in text
        and " install --bundle-url " in text
        and BOOTSTRAP_SELECTOR_NAME in text
        and ASSET_INDEX_NAME in text,
        "install.sh template must hand off to the versioned m80 install command",
    )


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
    guestd: dict,
    compatibility: dict,
    target_os: str,
    target_arch: str,
    files: list[dict],
) -> dict:
    return {
        "schema_version": BUNDLE_SCHEMA_VERSION,
        "release_tag": args.release_tag,
        "m80_version": version["binary_version"],
        "package_version": version["package_version"],
        "target": args.target,
        "os": target_os,
        "arch": target_arch,
        "image_kind": args.image_kind,
        "m80_protocol_version": compatibility["m80_protocol_version"],
        "guestd_package_version": guestd["package_version"],
        "guest_protocol_version": compatibility["guest_protocol_version"],
        "manifest_schema_version": compatibility["manifest_schema"],
        "build_receipt_schema_version": compatibility["build_receipt_schema"],
        "build_receipt_manifest_path": compatibility["build_receipt_manifest_path"],
        "install_provenance_schema_version": compatibility["install_provenance_schema"],
        "install_provenance_required": True,
        "expected_firecracker_version": compatibility["expected_firecracker_version"],
        "files": files,
    }


def release_asset_index(
    *,
    args: argparse.Namespace,
    version: dict,
    compatibility: dict,
    target_os: str,
    target_arch: str,
    tarball: Path,
    metadata_asset: Path,
) -> dict:
    return {
        "schema_version": ASSET_INDEX_SCHEMA_VERSION,
        "release_tag": args.release_tag,
        "assets": [
            {
                "name": BUNDLE_NAME,
                "url": release_asset_url(args.release_tag, BUNDLE_NAME),
                "sha256": sha256(tarball),
                "size_bytes": tarball.stat().st_size,
                "metadata_name": METADATA_NAME,
                "metadata_sha256": sha256(metadata_asset),
                "checksum_name": f"{BUNDLE_NAME}.sha256",
                "signature_name": None,
                "attestation_name": INTEGRITY_ATTESTATION_BUNDLE_NAME,
                "target": args.target,
                "os": target_os,
                "arch": target_arch,
                "image_kind": args.image_kind,
                "release_tag": args.release_tag,
                "m80_version": version["binary_version"],
                "guest_protocol_version": compatibility["guest_protocol_version"],
                "manifest_schema_version": compatibility["manifest_schema"],
                "expected_firecracker_version": compatibility["expected_firecracker_version"],
            }
        ],
    }


def release_build_manifest(
    *,
    args: argparse.Namespace,
    package_version: str,
    metadata_asset: Path,
    target_triples: list[str],
    apt_packages: list[dict[str, str]],
    container_digest: str | None,
    repo_root: Path,
) -> dict:
    return {
        "schema_version": BUILD_MANIFEST_SCHEMA_VERSION,
        "release_tag": args.release_tag,
        "source_commit": args.commit_sha,
        "rust_toolchain": args.rust_toolchain,
        "target": args.target,
        "target_triples": target_triples,
        "m80_package_version": package_version,
        "image_kind": args.image_kind,
        "cargo_lock_sha256": sha256(repo_root / "Cargo.lock"),
        "builder_identity": args.builder_identity,
        "builder_os_image": args.builder_os_image,
        "apt_packages": apt_packages,
        "container_digest": container_digest,
        "bundle_metadata_name": METADATA_NAME,
        "bundle_metadata_sha256": sha256(metadata_asset),
    }


def write_bootstrap_selector(path: Path, asset_index: dict) -> None:
    rows = [
        f"schema_version\t{BOOTSTRAP_SELECTOR_SCHEMA_VERSION}\n",
        f"release_tag\t{selector_value(asset_index['release_tag'], 'release_tag')}\n",
        "columns\t" + "\t".join(BOOTSTRAP_SELECTOR_COLUMNS) + "\n",
    ]
    for asset in asset_index["assets"]:
        rows.append(
            "row\t"
            + "\t".join(
                [
                    selector_value(asset["os"], "os"),
                    selector_value(asset["arch"], "arch"),
                    selector_value(asset["image_kind"], "image_kind"),
                    selector_value(asset["name"], "name"),
                    selector_value(asset["url"], "url"),
                    selector_value(asset["sha256"], "sha256"),
                    selector_value(asset["size_bytes"], "size_bytes"),
                    selector_value(asset["metadata_name"], "metadata_name"),
                    selector_value(asset["metadata_sha256"], "metadata_sha256"),
                    selector_value(asset["checksum_name"], "checksum_name"),
                    selector_value(asset["signature_name"], "signature_name"),
                    selector_value(asset["attestation_name"], "attestation_name"),
                    selector_value(asset["m80_version"], "m80_version"),
                ]
            )
            + "\n"
        )
    path.write_text("".join(rows))


def selector_value(value: object, field: str) -> str:
    if value is None:
        return "-"
    if isinstance(value, int):
        require(value > 0, f"bootstrap selector {field} must be greater than zero")
        return str(value)
    require(isinstance(value, str) and value, f"bootstrap selector {field} must not be empty")
    require(
        not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in value),
        f"bootstrap selector {field} must not contain whitespace or control characters",
    )
    require(
        SELECTOR_VALUE_RE.match(value) is not None,
        f"bootstrap selector {field} must contain only shell-safe token characters",
    )
    return value


def release_integrity_material(
    *,
    args: argparse.Namespace,
    package_version: str,
    metadata_asset: Path,
    dist_dir: Path,
) -> dict:
    return {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": release_repository(),
        "release_tag": args.release_tag,
        "commit_sha": args.commit_sha,
        "target": args.target,
        "rust_toolchain": args.rust_toolchain,
        "m80_package_version": package_version,
        "bundle_metadata_name": METADATA_NAME,
        "bundle_metadata_sha256": sha256(metadata_asset),
        "subjects": [
            release_integrity_subject(dist_dir / name, name, kind)
            for name, kind in INTEGRITY_SUBJECT_KINDS
        ],
    }


def release_integrity_subject(path: Path, name: str, kind: str) -> dict:
    require(path.is_file(), f"release integrity subject missing: {name}")
    return {
        "name": name,
        "kind": kind,
        "sha256": sha256(path),
        "size_bytes": path.stat().st_size,
    }


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def write_sha256s(path: Path, rows: list[dict]) -> None:
    lines = [f"{row['sha256']}  {row['path']}\n" for row in rows]
    path.write_text("".join(lines))


def write_sha256_sidecar(path: Path, asset: Path, asset_name: str) -> None:
    path.write_text(f"{sha256(asset)}  {asset_name}\n")
    path.chmod(0o644)


def write_public_sha256s(path: Path, assets: list[tuple[str, Path]]) -> None:
    lines = [f"{sha256(asset)}  {name}\n" for name, asset in assets]
    path.write_text("".join(lines))
    path.chmod(0o644)


def write_deterministic_tar_gz(root: Path, tarball: Path) -> None:
    with tarball.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as gzip_file:
            with tarfile.open(fileobj=gzip_file, mode="w", format=tarfile.USTAR_FORMAT) as tar:
                for path in sorted(root.rglob("*")):
                    if not path.is_file():
                        continue
                    rel = path.relative_to(root).as_posix()
                    data = path.read_bytes()
                    info = tarfile.TarInfo(rel)
                    info.size = len(data)
                    info.mode = FILE_MODES[rel]
                    info.mtime = 0
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    tar.addfile(info, io.BytesIO(data))


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
