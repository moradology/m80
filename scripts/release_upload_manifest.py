#!/usr/bin/env python3
"""Write and verify the public m80 release upload manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys


SCHEMA_VERSION = 1
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
INTEGRITY_NAME = "m80-release-integrity.json"
INTEGRITY_ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"
INTEGRITY_ATTESTATION_METADATA_NAME = "m80-release-attestation.json"
HOSTLESS_QUICKSTART_PROOF_NAME = "m80-quickstart-proof-hostless.json"
SHA256SUMS_NAME = "SHA256SUMS"

DIST_ASSET_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")

TOP_LEVEL_FIELDS = {
    "schema_version",
    "release_tag",
    "public_assets",
    "non_public_workflow_artifacts",
}
PUBLIC_ASSET_FIELDS = {
    "name",
    "kind",
    "sha256",
    "size_bytes",
    "integrity_subject",
}
NON_PUBLIC_FIELDS = {"name", "reason"}
INTEGRITY_SUBJECT_FIELDS = {"name", "kind", "sha256", "size_bytes"}

POST_PREDICATE_PUBLIC_PROOF_KINDS = {
    INTEGRITY_ATTESTATION_BUNDLE_NAME: "github-artifact-attestation-bundle",
    INTEGRITY_ATTESTATION_METADATA_NAME: "release-attestation-metadata",
}
NON_PUBLIC_WORKFLOW_ARTIFACTS = {
    UPLOAD_MANIFEST_NAME: "workflow-only source of public upload and redownload truth",
    HOSTLESS_QUICKSTART_PROOF_NAME: (
        "hostless quickstart proof is workflow evidence; public release proof is the "
        "release-integrity predicate plus attestation material"
    ),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-dir", required=True, type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument(
        "--manifest",
        type=Path,
        help=f"default: <dist-dir>/{UPLOAD_MANIFEST_NAME}",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="write the manifest before verifying it",
    )
    parser.add_argument(
        "--print-upload-paths",
        action="store_true",
        help="print public asset paths, one per line",
    )
    parser.add_argument(
        "--print-download-patterns",
        action="store_true",
        help="print public asset names, one per line",
    )
    parser.add_argument(
        "--require-exact-dist-public-assets",
        action="store_true",
        help="require dist-dir file names to equal public_assets exactly",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dist_dir = args.dist_dir.resolve()
    manifest_path = args.manifest or dist_dir / UPLOAD_MANIFEST_NAME

    if args.write:
        write_json(manifest_path, build_manifest(dist_dir, args.release_tag))

    manifest = read_json(manifest_path, "release upload manifest")
    verify_manifest(manifest, dist_dir, args.release_tag)
    if args.require_exact_dist_public_assets:
        verify_exact_dist_public_assets(manifest, dist_dir)

    if args.print_upload_paths:
        for asset in manifest["public_assets"]:
            print(dist_dir / asset["name"])
    if args.print_download_patterns:
        for asset in manifest["public_assets"]:
            print(asset["name"])
    return 0


def build_manifest(dist_dir: Path, release_tag: str) -> dict:
    public_assets = expected_public_assets(dist_dir, release_tag)
    return {
        "schema_version": SCHEMA_VERSION,
        "release_tag": release_tag,
        "public_assets": public_assets,
        "non_public_workflow_artifacts": [
            {"name": name, "reason": reason}
            for name, reason in NON_PUBLIC_WORKFLOW_ARTIFACTS.items()
        ],
    }


def verify_manifest(manifest: dict, dist_dir: Path, release_tag: str) -> None:
    require_exact_fields(manifest, TOP_LEVEL_FIELDS, "release upload manifest")
    require(
        manifest["schema_version"] == SCHEMA_VERSION,
        f"unsupported release upload manifest schema_version: expected {SCHEMA_VERSION}, got {manifest['schema_version']}",
    )
    require(manifest["release_tag"] == release_tag, "release upload manifest release_tag mismatch")

    expected_assets = expected_public_assets(dist_dir, release_tag)
    expected_by_name = {asset["name"]: asset for asset in expected_assets}
    public_assets = require_list(manifest["public_assets"], "release upload manifest public_assets")
    observed_by_name = unique_rows_by_name(public_assets, PUBLIC_ASSET_FIELDS, "release upload manifest public asset")
    require_name_set(
        set(observed_by_name),
        set(expected_by_name),
        "release upload manifest public asset set",
    )
    for name, expected in expected_by_name.items():
        observed = observed_by_name[name]
        for field, expected_value in expected.items():
            require(
                observed[field] == expected_value,
                f"release upload manifest public asset {name} {field} mismatch: "
                f"expected {expected_value!r}, got {observed[field]!r}",
            )

    material = read_integrity_material(dist_dir, release_tag)
    verify_sha256s_coverage(dist_dir, material["subjects"])
    integrity_subject_names = {subject["name"] for subject in material["subjects"]}
    observed_integrity_names = {
        asset["name"] for asset in public_assets if asset["integrity_subject"] is True
    }
    require_name_set(
        observed_integrity_names,
        integrity_subject_names,
        "release upload manifest integrity subject public assets",
    )
    observed_post_predicate = {
        asset["name"]
        for asset in public_assets
        if asset["kind"] in set(POST_PREDICATE_PUBLIC_PROOF_KINDS.values())
    }
    require_name_set(
        observed_post_predicate,
        set(POST_PREDICATE_PUBLIC_PROOF_KINDS),
        "release upload manifest post-predicate public proof assets",
    )

    non_public = require_list(
        manifest["non_public_workflow_artifacts"],
        "release upload manifest non_public_workflow_artifacts",
    )
    observed_non_public = unique_rows_by_name(
        non_public,
        NON_PUBLIC_FIELDS,
        "release upload manifest non-public workflow artifact",
    )
    require_name_set(
        set(observed_non_public),
        set(NON_PUBLIC_WORKFLOW_ARTIFACTS),
        "release upload manifest non-public workflow artifact set",
    )
    for name, reason in NON_PUBLIC_WORKFLOW_ARTIFACTS.items():
        require(
            observed_non_public[name]["reason"] == reason,
            f"release upload manifest non-public workflow artifact {name} reason mismatch",
        )


def verify_exact_dist_public_assets(manifest: dict, dist_dir: Path) -> None:
    public_assets = require_list(manifest["public_assets"], "release upload manifest public_assets")
    public_by_name = unique_rows_by_name(
        public_assets,
        PUBLIC_ASSET_FIELDS,
        "release upload manifest public asset",
    )
    actual_files = {path.name for path in dist_dir.iterdir() if path.is_file()}
    require_name_set(
        actual_files,
        set(public_by_name),
        "release upload redownload file set",
    )


def verify_sha256s_coverage(dist_dir: Path, subjects: list[object]) -> None:
    expected = {
        subject["name"]: subject["sha256"]
        for subject in subjects
        if subject["name"] != SHA256SUMS_NAME
    }
    sums = read_sha256s(dist_dir / SHA256SUMS_NAME)
    require_name_set(
        set(sums),
        set(expected),
        "release upload SHA256SUMS subject coverage",
    )
    for name, expected_sha in expected.items():
        require(
            sums[name] == expected_sha,
            f"release upload SHA256SUMS hash mismatch for {name}: "
            f"expected {expected_sha}, got {sums[name]}",
        )


def expected_public_assets(dist_dir: Path, release_tag: str) -> list[dict]:
    material = read_integrity_material(dist_dir, release_tag)
    rows: list[dict] = []
    seen: set[str] = set()
    for subject in material["subjects"]:
        require_exact_fields(subject, INTEGRITY_SUBJECT_FIELDS, "release integrity subject")
        name = require_dist_asset_name(subject["name"], "release integrity subject name")
        require(name not in seen, f"release integrity duplicate subject name: {name}")
        seen.add(name)
        require_sha256(subject["sha256"], f"release integrity subject {name} sha256")
        require_non_negative_int(subject["size_bytes"], f"release integrity subject {name} size_bytes")
        path = dist_dir / name
        require(path.is_file(), f"release upload public asset missing: {name}")
        actual_sha = sha256_file(path)
        actual_size = path.stat().st_size
        require(
            subject["sha256"] == actual_sha,
            f"release upload integrity subject {name} sha256 mismatch: expected {subject['sha256']}, got {actual_sha}",
        )
        require(
            subject["size_bytes"] == actual_size,
            f"release upload integrity subject {name} size mismatch: expected {subject['size_bytes']}, got {actual_size}",
        )
        rows.append(public_asset_row(path, name, subject["kind"], integrity_subject=True))

    rows.append(
        public_asset_row(
            dist_dir / INTEGRITY_NAME,
            INTEGRITY_NAME,
            "release-integrity-predicate",
            integrity_subject=False,
        )
    )
    for name, kind in POST_PREDICATE_PUBLIC_PROOF_KINDS.items():
        rows.append(public_asset_row(dist_dir / name, name, kind, integrity_subject=False))
    return rows


def public_asset_row(path: Path, name: str, kind: str, *, integrity_subject: bool) -> dict:
    require_dist_asset_name(name, "release upload public asset name")
    require(isinstance(kind, str) and kind, f"release upload public asset {name} kind must not be empty")
    require(path.is_file(), f"release upload public asset missing: {name}")
    return {
        "name": name,
        "kind": kind,
        "sha256": sha256_file(path),
        "size_bytes": path.stat().st_size,
        "integrity_subject": integrity_subject,
    }


def read_integrity_material(dist_dir: Path, release_tag: str) -> dict:
    material = read_json(dist_dir / INTEGRITY_NAME, "release integrity material")
    require(isinstance(material.get("subjects"), list), "release integrity material subjects must be a list")
    require(material.get("release_tag") == release_tag, "release integrity material release_tag mismatch")
    return material


def unique_rows_by_name(rows: list[object], fields: set[str], label: str) -> dict[str, dict]:
    by_name: dict[str, dict] = {}
    for row in rows:
        require(isinstance(row, dict), f"{label} must be an object")
        require_exact_fields(row, fields, label)
        name = require_dist_asset_name(row["name"], f"{label} name")
        require(name not in by_name, f"{label} duplicate name: {name}")
        if "sha256" in row:
            require_sha256(row["sha256"], f"{label} {name} sha256")
        if "size_bytes" in row:
            require_non_negative_int(row["size_bytes"], f"{label} {name} size_bytes")
        if "integrity_subject" in row:
            require(isinstance(row["integrity_subject"], bool), f"{label} {name} integrity_subject must be boolean")
        if "kind" in row:
            require(isinstance(row["kind"], str) and row["kind"], f"{label} {name} kind must not be empty")
        if "reason" in row:
            require(isinstance(row["reason"], str) and row["reason"], f"{label} {name} reason must not be empty")
        by_name[name] = row
    return by_name


def require_name_set(observed: set[str], expected: set[str], label: str) -> None:
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def comma_or_none(values: list[str]) -> str:
    return ", ".join(values) if values else "none"


def require_list(value: object, label: str) -> list[object]:
    require(isinstance(value, list), f"{label} must be a list")
    return value


def require_dist_asset_name(value: object, label: str) -> str:
    require(
        isinstance(value, str)
        and value
        and "/" not in value
        and value not in {".", ".."}
        and DIST_ASSET_NAME_RE.fullmatch(value) is not None,
        f"{label} must be a flat dist asset name",
    )
    return value


def require_sha256(value: object, label: str) -> None:
    require(isinstance(value, str) and SHA256_RE.fullmatch(value) is not None, f"{label} must be a lowercase sha256")


def require_non_negative_int(value: object, label: str) -> None:
    require(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")


def require_exact_fields(obj: dict, expected: set[str], label: str) -> None:
    observed = set(obj)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} field mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        payload = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{label} is not valid JSON: {exc}") from exc
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def read_sha256s(path: Path) -> dict[str, str]:
    require(path.is_file(), f"release upload SHA256SUMS missing: {path}")
    result: dict[str, str] = {}
    for line in path.read_text().splitlines():
        require(line, "release upload SHA256SUMS contains empty line")
        parts = line.split()
        require(len(parts) == 2, "release upload SHA256SUMS row shape invalid")
        digest, name = parts
        require_sha256(digest, f"release upload SHA256SUMS digest for {name}")
        require_dist_asset_name(name, "release upload SHA256SUMS asset name")
        require(name not in result, f"release upload SHA256SUMS duplicate asset: {name}")
        result[name] = digest
    require(result, "release upload SHA256SUMS must not be empty")
    return result


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
