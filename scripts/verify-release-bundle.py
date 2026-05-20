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
ASSET_INDEX_SCHEMA_VERSION = 1
SUPPORTED_TARGET = "linux-x86_64"
SUPPORTED_IMAGE_KIND = "minimal"
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
ASSET_INDEX_NAME = "m80-release-assets.json"
INSTALL_NAME = "install.sh"
GITHUB_RELEASE_BASE_URL = "https://github.com/moradology/m80/releases/download"
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
FORBIDDEN_INSTALL_TIME_PATHS = {"artifacts/host-binaries.manifest.json"}
FORBIDDEN_HOST_PREREQ_PATHS = {
    "bin/firecracker",
    "bin/jailer",
    "bin/firecracker-seccomp-filter.bin",
    "bin/firecracker-seccomp-filter.json",
    "artifacts/firecracker",
    "artifacts/jailer",
    "artifacts/firecracker-seccomp-filter.bin",
    "artifacts/firecracker-seccomp-filter.json",
    "firecracker",
    "jailer",
    "firecracker-seccomp-filter.bin",
    "firecracker-seccomp-filter.json",
}
HOST_PREREQ_POLICY_MESSAGE = (
    "m80 v0.x release bundles own m80 binaries/helpers and guest artifacts; "
    "official Firecracker, official jailer, and Firecracker seccomp filter "
    "payloads are operator-provided host prerequisites"
)
EXPECTED_MODES = {
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
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--target", default="linux-x86_64")
    parser.add_argument("--image-kind", default="minimal")
    parser.add_argument("--repo-root", default=Path.cwd(), type=Path)
    parser.add_argument(
        "--verify-sidecars",
        action="store_true",
        help="verify adjacent public release sidecars emitted by package-release-bundle.py",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    target_os, target_arch = supported_target_parts(args.target)
    require(
        args.image_kind == SUPPORTED_IMAGE_KIND,
        f"unsupported image kind: expected {SUPPORTED_IMAGE_KIND}, got {args.image_kind}",
    )
    workspace_version = workspace_package_version(args.repo_root)
    expected_tag = f"v{workspace_version}"
    require(args.release_tag == expected_tag, f"release tag mismatch: expected {expected_tag}, got {args.release_tag}")

    with tarfile.open(args.bundle, "r:gz") as tar:
        files = read_regular_files(tar)

    paths = set(files)
    missing = sorted(REQUIRED_PATHS - paths)
    require(not missing, f"bundle missing required paths: {', '.join(missing)}")
    forbidden = sorted(FORBIDDEN_INSTALL_TIME_PATHS & paths)
    require(not forbidden, f"bundle contains install-time-only paths: {', '.join(forbidden)}")
    forbidden_host_prereqs = sorted(FORBIDDEN_HOST_PREREQ_PATHS & paths)
    require(
        not forbidden_host_prereqs,
        "bundle contains operator-provided host prerequisite payloads: "
        f"{', '.join(forbidden_host_prereqs)}; {HOST_PREREQ_POLICY_MESSAGE}",
    )
    unexpected = sorted(paths - REQUIRED_PATHS)
    require(not unexpected, f"bundle contains unexpected paths: {', '.join(unexpected)}")
    for path, expected_mode in EXPECTED_MODES.items():
        actual_mode = files[path]["mode"]
        require(
            actual_mode == expected_mode,
            f"bundle mode mismatch for {path}: expected {expected_mode:o}, got {actual_mode:o}",
        )

    metadata = json.loads(files["bundle.json"]["data"].decode("utf-8"))
    require(metadata.get("schema_version") == BUNDLE_SCHEMA_VERSION, "unsupported bundle schema_version")
    require(metadata.get("release_tag") == args.release_tag, "bundle release_tag mismatch")
    require(metadata.get("m80_version") == args.release_tag, "bundle m80_version mismatch")
    require(metadata.get("package_version") == workspace_version, "bundle package_version mismatch")
    require(metadata.get("guestd_package_version") == workspace_version, "bundle guestd_package_version mismatch")
    require(metadata.get("target") == args.target, "bundle target mismatch")
    require(metadata.get("os") == target_os, "bundle os mismatch")
    require(metadata.get("arch") == target_arch, "bundle arch mismatch")
    require(metadata.get("image_kind") == args.image_kind, "bundle image_kind mismatch")
    require(isinstance(metadata.get("manifest_schema_version"), int), "bundle manifest_schema_version missing")
    require(
        isinstance(metadata.get("build_receipt_schema_version"), int),
        "bundle build_receipt_schema_version missing",
    )
    require(metadata.get("build_receipt_manifest_path"), "bundle build_receipt_manifest_path missing")
    require(
        isinstance(metadata.get("install_provenance_schema_version"), int),
        "bundle install_provenance_schema_version missing",
    )
    require(metadata.get("install_provenance_required") is True, "bundle install_provenance_required missing")
    require(isinstance(metadata.get("m80_protocol_version"), int), "bundle m80_protocol_version missing")
    require(isinstance(metadata.get("guest_protocol_version"), int), "bundle guest_protocol_version missing")
    require(
        metadata.get("m80_protocol_version") == metadata.get("guest_protocol_version"),
        "bundle protocol version mismatch",
    )
    require(metadata.get("expected_firecracker_version"), "bundle expected_firecracker_version missing")

    verify_compatibility_tuple(files, metadata)

    metadata_files = metadata_file_map(metadata)
    require(set(metadata_files) == PAYLOAD_PATHS, "bundle metadata file set mismatch")
    for path, expected in metadata_files.items():
        actual = sha256_bytes(files[path]["data"])
        require(actual == expected, f"bundle metadata hash mismatch for {path}")

    sums = parse_sha256s(files["SHA256SUMS"]["data"].decode("utf-8"))
    require(set(sums) == PAYLOAD_PATHS | {"bundle.json"}, "SHA256SUMS file set mismatch")
    for path, expected in sums.items():
        actual = sha256_bytes(files[path]["data"])
        require(actual == expected, f"SHA256SUMS hash mismatch for {path}")

    if args.verify_sidecars:
        verify_sidecars(
            args.bundle,
            files["bundle.json"]["data"],
            metadata,
            release_tag=args.release_tag,
            target=args.target,
            image_kind=args.image_kind,
            target_os=target_os,
            target_arch=target_arch,
        )

    print(f"verified {args.bundle}")
    return 0


def read_regular_files(tar: tarfile.TarFile) -> dict[str, dict]:
    files: dict[str, dict] = {}
    for member in tar.getmembers():
        name = member.name
        require(not name.startswith("/"), f"bundle path must be relative: {name}")
        require(".." not in Path(name).parts, f"bundle path must not escape root: {name}")
        require(member.isfile(), f"bundle contains non-file entry: {name}")
        require(name not in files, f"bundle duplicate path: {name}")
        extracted = tar.extractfile(member)
        require(extracted is not None, f"bundle member unreadable: {name}")
        files[name] = {"data": extracted.read(), "mode": member.mode & 0o777}
    return files


def supported_target_parts(target: str) -> tuple[str, str]:
    require(target == SUPPORTED_TARGET, f"unsupported target: expected {SUPPORTED_TARGET}, got {target}")
    return ("linux", "x86_64")


def verify_compatibility_tuple(files: dict[str, dict], metadata: dict) -> None:
    guest_manifest = json.loads(files["artifacts/output.ext4.manifest.json"]["data"].decode("utf-8"))
    receipt = json.loads(files["artifacts/output.ext4.build-receipt.json"]["data"].decode("utf-8"))

    require(
        metadata["manifest_schema_version"] == guest_manifest.get("schema_version"),
        "bundle manifest_schema_version mismatch",
    )
    require(
        metadata["build_receipt_schema_version"] == receipt.get("schema_version"),
        "bundle build_receipt_schema_version mismatch",
    )
    require(
        metadata["expected_firecracker_version"] == guest_manifest.get("expected_firecracker_version"),
        "bundle expected_firecracker_version mismatch",
    )
    require(metadata["image_kind"] == guest_manifest.get("image_kind"), "bundle image_kind/manifest mismatch")
    require(
        guest_manifest.get("source_rootfs_image") is None and guest_manifest.get("source_rootfs_sha256") is None,
        "minimal guest manifest must not record source rootfs artifacts",
    )
    require(
        receipt.get("manifest_sha256") == sha256_bytes(files["artifacts/output.ext4.manifest.json"]["data"]),
        "build receipt manifest_sha256 mismatch",
    )
    receipt_manifest_path = Path(require_str(receipt, "manifest_path", "build receipt"))
    require(
        metadata["build_receipt_manifest_path"] == str(receipt_manifest_path),
        "bundle build_receipt_manifest_path mismatch",
    )
    require(
        receipt_manifest_path.name == "output.ext4.manifest.json",
        "build receipt manifest_path mismatch",
    )

    artifact_rows = receipt_artifact_map(receipt)
    require(
        set(artifact_rows) == {"kernel_image", "output_rootfs_image", "daemon_binary_path"},
        "build receipt artifact set mismatch",
    )
    verify_manifest_artifact(
        files,
        guest_manifest,
        artifact_rows,
        "artifacts/vmlinux",
        "kernel_image",
        "kernel_image_sha256",
    )
    verify_manifest_artifact(
        files,
        guest_manifest,
        artifact_rows,
        "artifacts/output.ext4",
        "output_rootfs_image",
        "output_rootfs_sha256",
    )
    verify_manifest_artifact(
        files,
        guest_manifest,
        artifact_rows,
        "artifacts/m80-guestd",
        "daemon_binary_path",
        "daemon_binary_sha256",
    )


def verify_manifest_artifact(
    files: dict[str, dict],
    guest_manifest: dict,
    artifact_rows: dict[str, dict],
    bundle_path: str,
    manifest_path_field: str,
    manifest_sha_field: str,
) -> None:
    manifest_path = require_str(guest_manifest, manifest_path_field, "guest manifest")
    manifest_sha = require_str(guest_manifest, manifest_sha_field, "guest manifest")
    require(manifest_sha == sha256_bytes(files[bundle_path]["data"]), f"guest manifest {manifest_sha_field} mismatch")
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


def require_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} missing {key}")
    return value


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


def verify_sidecars(
    bundle: Path,
    bundle_metadata: bytes,
    metadata: dict,
    *,
    release_tag: str,
    target: str,
    image_kind: str,
    target_os: str,
    target_arch: str,
) -> None:
    sidecar_dir = bundle.parent
    require(bundle.name == BUNDLE_NAME, f"bundle filename mismatch: expected {BUNDLE_NAME}, got {bundle.name}")
    install_asset = sidecar_dir / INSTALL_NAME
    metadata_asset = sidecar_dir / METADATA_NAME
    asset_index = sidecar_dir / ASSET_INDEX_NAME
    public_sums = sidecar_dir / "SHA256SUMS"
    require(install_asset.is_file(), f"missing public sidecar: {INSTALL_NAME}")
    require(metadata_asset.is_file(), f"missing public sidecar: {METADATA_NAME}")
    require(asset_index.is_file(), f"missing public sidecar: {ASSET_INDEX_NAME}")
    require(public_sums.is_file(), "missing public sidecar: SHA256SUMS")
    verify_public_mode(bundle, 0o644)
    verify_public_mode(install_asset, 0o755)
    verify_public_mode(metadata_asset, 0o644)
    verify_public_mode(asset_index, 0o644)
    verify_public_mode(public_sums, 0o644)
    require(metadata_asset.read_bytes() == bundle_metadata, f"{METADATA_NAME} does not match bundled bundle.json")

    expected_assets = {
        BUNDLE_NAME: bundle,
        INSTALL_NAME: install_asset,
        METADATA_NAME: metadata_asset,
        ASSET_INDEX_NAME: asset_index,
    }
    for name, path in expected_assets.items():
        verify_single_sha256(sidecar_dir / f"{name}.sha256", name, path)

    sums = parse_sha256s(public_sums.read_text())
    require(set(sums) == set(expected_assets), "public SHA256SUMS file set mismatch")
    for name, path in expected_assets.items():
        require(sums[name] == sha256_file(path), f"public SHA256SUMS hash mismatch for {name}")
    verify_asset_index(
        asset_index,
        bundle=bundle,
        metadata_asset=metadata_asset,
        metadata=metadata,
        release_tag=release_tag,
        target=target,
        image_kind=image_kind,
        target_os=target_os,
        target_arch=target_arch,
    )


def verify_asset_index(
    asset_index: Path,
    *,
    bundle: Path,
    metadata_asset: Path,
    metadata: dict,
    release_tag: str,
    target: str,
    image_kind: str,
    target_os: str,
    target_arch: str,
) -> None:
    index = json.loads(asset_index.read_text())
    require(index.get("schema_version") == ASSET_INDEX_SCHEMA_VERSION, "unsupported asset index schema_version")
    require(index.get("release_tag") == release_tag, "asset index release_tag mismatch")
    assets = index.get("assets")
    require(isinstance(assets, list), "asset index assets must be a list")
    default_assets = [
        asset
        for asset in assets
        if isinstance(asset, dict)
        and asset.get("os") == target_os
        and asset.get("arch") == target_arch
        and asset.get("image_kind") == image_kind
    ]
    require(default_assets, "asset index missing default bundle")
    require(len(default_assets) == 1, "asset index duplicate default bundle")
    asset = default_assets[0]
    expected = {
        "name": BUNDLE_NAME,
        "url": release_asset_url(release_tag, BUNDLE_NAME),
        "sha256": sha256_file(bundle),
        "size_bytes": bundle.stat().st_size,
        "metadata_name": METADATA_NAME,
        "metadata_sha256": sha256_file(metadata_asset),
        "checksum_name": f"{BUNDLE_NAME}.sha256",
        "target": target,
        "os": target_os,
        "arch": target_arch,
        "image_kind": image_kind,
        "release_tag": release_tag,
        "m80_version": metadata["m80_version"],
        "guest_protocol_version": metadata["guest_protocol_version"],
        "manifest_schema_version": metadata["manifest_schema_version"],
        "expected_firecracker_version": metadata["expected_firecracker_version"],
    }
    for field, expected_value in expected.items():
        require(asset.get(field) == expected_value, f"asset index {field} mismatch")
    if asset.get("signature_name") is not None:
        require(isinstance(asset.get("signature_name"), str) and asset["signature_name"], "asset index signature_name invalid")
    if asset.get("attestation_name") is not None:
        require(
            isinstance(asset.get("attestation_name"), str) and asset["attestation_name"],
            "asset index attestation_name invalid",
        )


def release_asset_url(release_tag: str, asset_name: str) -> str:
    return f"{GITHUB_RELEASE_BASE_URL}/{release_tag}/{asset_name}"


def verify_single_sha256(sidecar: Path, expected_name: str, asset: Path) -> None:
    require(sidecar.is_file(), f"missing checksum sidecar: {sidecar.name}")
    verify_public_mode(sidecar, 0o644)
    sums = parse_sha256s(sidecar.read_text())
    require(sums == {expected_name: sha256_file(asset)}, f"checksum sidecar mismatch for {expected_name}")


def verify_public_mode(path: Path, expected_mode: int) -> None:
    actual_mode = path.stat().st_mode & 0o777
    require(
        actual_mode == expected_mode,
        f"public sidecar mode mismatch for {path.name}: expected {expected_mode:o}, got {actual_mode:o}",
    )


def workspace_package_version(repo_root: Path) -> str:
    with (repo_root / "Cargo.toml").open("rb") as f:
        cargo = tomllib.load(f)
    return cargo["workspace"]["package"]["version"]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
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
