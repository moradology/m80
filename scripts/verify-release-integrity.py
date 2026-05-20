#!/usr/bin/env python3
"""Verify the m80 release integrity material contract."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import tomllib


SCHEMA_VERSION = 1
MECHANISM = "github-artifact-attestation"
REPOSITORY = "moradology/m80"
TARGET = "linux-x86_64"
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
ASSET_INDEX_NAME = "m80-release-assets.json"
INSTALL_NAME = "install.sh"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
TOP_LEVEL_FIELDS = {
    "schema_version",
    "mechanism",
    "repository",
    "release_tag",
    "commit_sha",
    "target",
    "rust_toolchain",
    "m80_package_version",
    "bundle_metadata_name",
    "bundle_metadata_sha256",
    "subjects",
}
SUBJECT_FIELDS = {"name", "kind", "sha256", "size_bytes"}
EXPECTED_SUBJECTS = {
    BUNDLE_NAME: "release-bundle",
    f"{BUNDLE_NAME}.sha256": "checksum-sidecar",
    INSTALL_NAME: "installer",
    f"{INSTALL_NAME}.sha256": "checksum-sidecar",
    METADATA_NAME: "bundle-metadata",
    f"{METADATA_NAME}.sha256": "checksum-sidecar",
    ASSET_INDEX_NAME: "asset-index",
    f"{ASSET_INDEX_NAME}.sha256": "checksum-sidecar",
    "SHA256SUMS": "checksum-manifest",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("material", type=Path)
    parser.add_argument("--dist-dir", type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--target", default=TARGET)
    parser.add_argument("--rust-toolchain")
    parser.add_argument("--repo-root", default=Path.cwd(), type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dist_dir = args.dist_dir or args.material.parent
    require(dist_dir.is_dir(), f"release dist dir missing: {dist_dir}")
    require(COMMIT_RE.match(args.commit_sha) is not None, "commit sha must be a 40-character lowercase hex digest")

    material = read_json(args.material, "release integrity material")
    require_exact_fields(material, TOP_LEVEL_FIELDS, "release integrity material")
    require(
        material["schema_version"] == SCHEMA_VERSION,
        f"unsupported release integrity schema_version: expected {SCHEMA_VERSION}, got {material['schema_version']}",
    )
    require(
        material["mechanism"] == MECHANISM,
        f"release integrity mechanism mismatch: expected {MECHANISM}, got {material['mechanism']}",
    )
    require(
        material["repository"] == REPOSITORY,
        f"release integrity repository mismatch: expected {REPOSITORY}, got {material['repository']}",
    )
    require(
        material["release_tag"] == args.release_tag,
        f"release integrity release_tag mismatch: expected {args.release_tag}, got {material['release_tag']}",
    )
    require(
        material["commit_sha"] == args.commit_sha,
        f"release integrity commit_sha mismatch: expected {args.commit_sha}, got {material['commit_sha']}",
    )
    require(
        material["target"] == args.target,
        f"release integrity target mismatch: expected {args.target}, got {material['target']}",
    )
    if args.rust_toolchain is not None:
        require(
            material["rust_toolchain"] == args.rust_toolchain,
            "release integrity rust_toolchain mismatch: "
            f"expected {args.rust_toolchain}, got {material['rust_toolchain']}",
        )
    else:
        require_nonempty_str(material, "rust_toolchain", "release integrity material")
    package_version = workspace_package_version(args.repo_root)
    require(
        material["m80_package_version"] == package_version,
        "release integrity m80_package_version mismatch: "
        f"expected {package_version}, got {material['m80_package_version']}",
    )
    require(
        material["bundle_metadata_name"] == METADATA_NAME,
        f"release integrity bundle_metadata_name mismatch: expected {METADATA_NAME}, got {material['bundle_metadata_name']}",
    )

    metadata_path = dist_dir / METADATA_NAME
    metadata_sha = sha256_file(metadata_path)
    require_valid_sha(material["bundle_metadata_sha256"], "release integrity bundle_metadata_sha256")
    require(
        material["bundle_metadata_sha256"] == metadata_sha,
        f"release integrity bundle_metadata_sha256 mismatch for {METADATA_NAME}",
    )
    verify_bundle_metadata(metadata_path, material)
    verify_asset_index(dist_dir / ASSET_INDEX_NAME, material)
    verify_subjects(material["subjects"], dist_dir)

    print(f"verified release integrity material {args.material}")
    return 0


def verify_bundle_metadata(metadata_path: Path, material: dict) -> None:
    metadata = read_json(metadata_path, "bundle metadata")
    require(
        metadata.get("release_tag") == material["release_tag"],
        "release integrity bundle metadata release_tag mismatch",
    )
    require(
        metadata.get("m80_version") == material["release_tag"],
        "release integrity bundle metadata m80_version mismatch",
    )
    require(
        metadata.get("package_version") == material["m80_package_version"],
        "release integrity bundle metadata package_version mismatch",
    )
    require(metadata.get("target") == material["target"], "release integrity bundle metadata target mismatch")


def verify_asset_index(index_path: Path, material: dict) -> None:
    index = read_json(index_path, "asset index")
    require(index.get("release_tag") == material["release_tag"], "release integrity asset index release_tag mismatch")


def verify_subjects(subjects: object, dist_dir: Path) -> None:
    require(isinstance(subjects, list), "release integrity subjects must be a list")
    by_name: dict[str, dict] = {}
    for subject in subjects:
        require(isinstance(subject, dict), "release integrity subject must be an object")
        name = require_subject_str(subject, "name", "<unknown>")
        require(name not in by_name, f"release integrity duplicate subject {name}")
        require_exact_fields(subject, SUBJECT_FIELDS, f"release integrity subject {name}")
        kind = require_subject_str(subject, "kind", name)
        require(kind == EXPECTED_SUBJECTS.get(name), f"release integrity subject {name} kind mismatch")
        digest = require_subject_str(subject, "sha256", name)
        require_valid_sha(digest, f"release integrity subject {name} sha256")
        size_bytes = subject["size_bytes"]
        require(isinstance(size_bytes, int) and size_bytes > 0, f"release integrity subject {name} invalid size_bytes")
        by_name[name] = subject

    require(set(by_name) == set(EXPECTED_SUBJECTS), "release integrity subject set mismatch")
    for name, subject in by_name.items():
        asset = dist_dir / name
        require(asset.is_file(), f"release integrity subject file missing: {name}")
        actual_sha = sha256_file(asset)
        require(
            subject["sha256"] == actual_sha,
            f"release integrity sha256 mismatch for {name}: expected {subject['sha256']}, got {actual_sha}",
        )
        actual_size = asset.stat().st_size
        require(
            subject["size_bytes"] == actual_size,
            f"release integrity size mismatch for {name}: expected {subject['size_bytes']}, got {actual_size}",
        )


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    with path.open() as f:
        payload = json.load(f)
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def require_exact_fields(obj: dict, expected: set[str], label: str) -> None:
    actual = set(obj)
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    require(not missing, f"{label} missing field(s): {', '.join(missing)}")
    require(not extra, f"{label} has unknown field(s): {', '.join(extra)}")


def require_nonempty_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} missing {key}")
    return value


def require_subject_str(subject: dict, key: str, name: str) -> str:
    value = subject.get(key)
    require(isinstance(value, str) and value, f"release integrity subject {name} missing {key}")
    return value


def require_valid_sha(value: object, label: str) -> None:
    require(isinstance(value, str) and SHA256_RE.match(value) is not None, f"{label} must be lowercase sha256")


def workspace_package_version(repo_root: Path) -> str:
    with (repo_root / "Cargo.toml").open("rb") as f:
        cargo = tomllib.load(f)
    return cargo["workspace"]["package"]["version"]


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
