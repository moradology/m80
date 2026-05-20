#!/usr/bin/env python3
"""Verify the m80 release integrity material contract."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

from release_attestation_verifier import preflight_gh_attestation_verifier
from release_url_contract import release_asset_url, release_repository


SCHEMA_VERSION = 1
BOOTSTRAP_SELECTOR_SCHEMA_VERSION = 1
MECHANISM = "github-artifact-attestation"
REPOSITORY = release_repository()
TARGET = "linux-x86_64"
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
ASSET_INDEX_NAME = "m80-release-assets.json"
BOOTSTRAP_SELECTOR_NAME = "m80-bootstrap-selector.tsv"
INSTALL_NAME = "install.sh"
INTEGRITY_ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"
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
DIST_ASSET_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
ASSET_INDEX_FIELDS = {"schema_version", "release_tag", "assets"}
ASSET_FIELDS = {
    "name",
    "url",
    "sha256",
    "size_bytes",
    "metadata_name",
    "metadata_sha256",
    "checksum_name",
    "signature_name",
    "attestation_name",
    "target",
    "os",
    "arch",
    "image_kind",
    "release_tag",
    "m80_version",
    "guest_protocol_version",
    "manifest_schema_version",
    "expected_firecracker_version",
}
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
TRUST_POLICY_FIELDS = {
    "schema_version",
    "mechanism",
    "repository",
    "keyset_id",
    "valid_from",
    "valid_until",
    "allowed_signers",
    "rotation",
}
TRUST_SIGNER_FIELDS = {"identity", "issuer"}
TRUST_ROTATION_FIELDS = {"mode", "overlap_days", "next_keyset_id"}
ATTESTATION_FIELDS = {
    "schema_version",
    "mechanism",
    "repository",
    "release_tag",
    "predicate_sha256",
    "signer_identity",
    "issuer",
    "keyset_id",
    "certificate_not_before",
    "certificate_not_after",
}
EXPECTED_SUBJECTS = {
    BUNDLE_NAME: "release-bundle",
    f"{BUNDLE_NAME}.sha256": "checksum-sidecar",
    INSTALL_NAME: "installer",
    f"{INSTALL_NAME}.sha256": "checksum-sidecar",
    METADATA_NAME: "bundle-metadata",
    f"{METADATA_NAME}.sha256": "checksum-sidecar",
    ASSET_INDEX_NAME: "asset-index",
    f"{ASSET_INDEX_NAME}.sha256": "checksum-sidecar",
    BOOTSTRAP_SELECTOR_NAME: "bootstrap-selector",
    f"{BOOTSTRAP_SELECTOR_NAME}.sha256": "checksum-sidecar",
    "SHA256SUMS": "checksum-manifest",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("material", type=Path)
    parser.add_argument("--dist-dir", type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--trust-policy", required=True, type=Path)
    parser.add_argument("--attestation-bundle", required=True, type=Path)
    parser.add_argument("--attestation-metadata", required=True, type=Path)
    parser.add_argument("--verification-time", required=True)
    parser.add_argument("--gh-bin", default="gh")
    parser.add_argument("--target", default=TARGET)
    parser.add_argument("--rust-toolchain")
    parser.add_argument("--repo-root", default=Path.cwd(), type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    preflight_gh_attestation_verifier(args.gh_bin)
    dist_dir = args.dist_dir or args.material.parent
    require(dist_dir.is_dir(), f"release dist dir missing: {dist_dir}")
    require(COMMIT_RE.match(args.commit_sha) is not None, "commit sha must be a 40-character lowercase hex digest")

    material = read_json(args.material, "release integrity material")
    require_exact_fields(material, TOP_LEVEL_FIELDS, "release integrity material")
    verification_time = parse_timestamp(args.verification_time, "verification time")
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
    verify_trust_anchor(
        material,
        material_path=args.material,
        trust_policy_path=args.trust_policy,
        attestation_bundle_path=args.attestation_bundle,
        attestation_path=args.attestation_metadata,
        gh_bin=args.gh_bin,
        release_tag=args.release_tag,
        commit_sha=args.commit_sha,
        verification_time=verification_time,
    )

    metadata_path = dist_dir / METADATA_NAME
    metadata_sha = sha256_file(metadata_path)
    require_valid_sha(material["bundle_metadata_sha256"], "release integrity bundle_metadata_sha256")
    require(
        material["bundle_metadata_sha256"] == metadata_sha,
        f"release integrity bundle_metadata_sha256 mismatch for {METADATA_NAME}",
    )
    verify_bundle_metadata(metadata_path, material)
    asset_index = verify_asset_index(
        dist_dir / ASSET_INDEX_NAME,
        material,
        dist_dir=dist_dir,
        attestation_bundle_path=args.attestation_bundle,
        metadata=read_json(metadata_path, "bundle metadata"),
    )
    verify_bootstrap_selector(dist_dir / BOOTSTRAP_SELECTOR_NAME, material, asset_index)
    verify_subjects(material["subjects"], dist_dir, signature_subjects_from_index(asset_index))

    print(f"verified release integrity material {args.material}")
    return 0


def verify_trust_anchor(
    material: dict,
    *,
    material_path: Path,
    trust_policy_path: Path,
    attestation_bundle_path: Path,
    attestation_path: Path,
    gh_bin: str,
    release_tag: str,
    commit_sha: str,
    verification_time: datetime,
) -> None:
    policy = read_json(trust_policy_path, "release trust policy")
    attestation = read_json(attestation_path, "release attestation metadata")
    require_exact_fields(policy, TRUST_POLICY_FIELDS, "release trust policy")
    require_exact_fields(attestation, ATTESTATION_FIELDS, "release attestation metadata")
    require(
        policy["schema_version"] == SCHEMA_VERSION,
        f"unsupported release trust policy schema_version: expected {SCHEMA_VERSION}, got {policy['schema_version']}",
    )
    require(
        attestation["schema_version"] == SCHEMA_VERSION,
        f"unsupported release attestation metadata schema_version: expected {SCHEMA_VERSION}, got {attestation['schema_version']}",
    )
    require(policy["mechanism"] == MECHANISM, f"release trust mechanism mismatch: expected {MECHANISM}")
    require(attestation["mechanism"] == MECHANISM, f"release attestation mechanism mismatch: expected {MECHANISM}")
    require(policy["repository"] == REPOSITORY, f"release trust repository mismatch: expected {REPOSITORY}")
    require(attestation["repository"] == material["repository"], "release trust attestation repository mismatch")
    require(attestation["release_tag"] == release_tag, "release trust attestation release_tag mismatch")
    require(attestation["release_tag"] == material["release_tag"], "release trust predicate tag mismatch")
    require_valid_sha(attestation["predicate_sha256"], "release attestation predicate_sha256")
    require(
        attestation["predicate_sha256"] == sha256_file(material_path),
        "release trust predicate digest mismatch",
    )
    policy_keyset_id = require_nonempty_str(policy, "keyset_id", "release trust policy")
    attestation_keyset_id = require_nonempty_str(attestation, "keyset_id", "release attestation metadata")
    require_nonempty_str(attestation, "signer_identity", "release attestation metadata")
    require_nonempty_str(attestation, "issuer", "release attestation metadata")
    require(policy_keyset_id == attestation_keyset_id, "release trust keyset mismatch")
    validate_policy_window(policy, verification_time)
    validate_certificate_window(attestation, verification_time)
    signers = allowed_signers(policy)
    metadata_signer = validate_signer(signers, attestation)
    verified_signer = verify_cryptographic_attestation(
        gh_bin=gh_bin,
        material_path=material_path,
        attestation_bundle_path=attestation_bundle_path,
        signers=signers,
        release_tag=release_tag,
        commit_sha=commit_sha,
    )
    require(metadata_signer == verified_signer, "release trust metadata signer mismatch")


def validate_policy_window(policy: dict, verification_time: datetime) -> None:
    valid_from = parse_timestamp(policy["valid_from"], "release trust valid_from")
    valid_until = parse_timestamp(policy["valid_until"], "release trust valid_until")
    require(valid_from <= valid_until, "release trust validity window is inverted")
    require(valid_from <= verification_time, "release trust policy is not active yet")
    require(verification_time <= valid_until, "release trust policy expired")
    rotation = policy["rotation"]
    require(isinstance(rotation, dict), "release trust rotation must be an object")
    require_exact_fields(rotation, TRUST_ROTATION_FIELDS, "release trust rotation")
    require(rotation["mode"] == "hard-fail-expired", "release trust rotation mode mismatch")
    require(
        type(rotation["overlap_days"]) is int and rotation["overlap_days"] >= 0,
        "release trust rotation overlap_days invalid",
    )
    require(
        rotation["next_keyset_id"] is None
        or (isinstance(rotation["next_keyset_id"], str) and rotation["next_keyset_id"]),
        "release trust rotation next_keyset_id invalid",
    )


def validate_certificate_window(attestation: dict, verification_time: datetime) -> None:
    not_before = parse_timestamp(
        attestation["certificate_not_before"],
        "release attestation certificate_not_before",
    )
    not_after = parse_timestamp(
        attestation["certificate_not_after"],
        "release attestation certificate_not_after",
    )
    require(not_before <= not_after, "release attestation certificate window is inverted")
    require(not_before <= verification_time, "release trust certificate is not active yet")
    require(verification_time <= not_after, "release trust certificate expired")


def allowed_signers(policy: dict) -> list[tuple[str, str]]:
    signers = policy["allowed_signers"]
    require(isinstance(signers, list), "release trust allowed_signers must be a list")
    allowed = []
    for signer in signers:
        require(isinstance(signer, dict), "release trust allowed signer must be an object")
        require_exact_fields(signer, TRUST_SIGNER_FIELDS, "release trust allowed signer")
        identity = require_nonempty_str(signer, "identity", "release trust allowed signer")
        issuer = require_nonempty_str(signer, "issuer", "release trust allowed signer")
        allowed.append((identity, issuer))
    require(allowed, "release trust allowed_signers must not be empty")
    return allowed


def validate_signer(signers: list[tuple[str, str]], attestation: dict) -> tuple[str, str]:
    metadata_signer = (attestation["signer_identity"], attestation["issuer"])
    require(
        metadata_signer in set(signers),
        "release trust signer not allowed",
    )
    return metadata_signer


def verify_cryptographic_attestation(
    *,
    gh_bin: str,
    material_path: Path,
    attestation_bundle_path: Path,
    signers: list[tuple[str, str]],
    release_tag: str,
    commit_sha: str,
) -> tuple[str, str]:
    require(attestation_bundle_path.is_file(), f"release attestation bundle missing: {attestation_bundle_path}")
    failures = []
    for signer_identity, signer_issuer in signers:
        if run_gh_attestation_verify(
            gh_bin=gh_bin,
            material_path=material_path,
            attestation_bundle_path=attestation_bundle_path,
            signer_identity=signer_identity,
            signer_issuer=signer_issuer,
            release_tag=release_tag,
            commit_sha=commit_sha,
        ):
            return signer_identity, signer_issuer
        failures.append(f"{signer_identity} / {signer_issuer}")
    raise SystemExit(
        "release trust cryptographic attestation verification failed for allowed signer(s): "
        + ", ".join(failures)
    )


def run_gh_attestation_verify(
    *,
    gh_bin: str,
    material_path: Path,
    attestation_bundle_path: Path,
    signer_identity: str,
    signer_issuer: str,
    release_tag: str,
    commit_sha: str,
) -> bool:
    cmd = [
        gh_bin,
        "attestation",
        "verify",
        str(material_path),
        "--repo",
        REPOSITORY,
        "--bundle",
        str(attestation_bundle_path),
        "--signer-workflow",
        signer_identity,
        "--cert-oidc-issuer",
        signer_issuer,
        "--source-ref",
        f"refs/tags/{release_tag}",
        "--source-digest",
        commit_sha,
        "--deny-self-hosted-runners",
        "--format",
        "json",
    ]
    try:
        completed = subprocess.run(cmd, check=False, text=True, capture_output=True)
    except FileNotFoundError as exc:
        raise SystemExit(f"release attestation verifier missing: {gh_bin}") from exc
    if completed.returncode != 0:
        return False
    require(completed.stdout.strip(), "release attestation verifier returned empty JSON")
    try:
        verified = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise SystemExit("release attestation verifier returned invalid JSON") from exc
    require(isinstance(verified, list) and verified, "release attestation verifier returned no attestations")
    require_gh_output_names_material(verified, material_path)
    return True


def require_gh_output_names_material(verified: list, material_path: Path) -> None:
    expected_sha = sha256_file(material_path)
    expected_names = {str(material_path), material_path.name}
    for entry in verified:
        require(isinstance(entry, dict), "release attestation verifier result must be an object")
        result = entry.get("verificationResult")
        if not isinstance(result, dict):
            continue
        statement = result.get("statement")
        if not isinstance(statement, dict):
            continue
        subjects = statement.get("subject")
        if not isinstance(subjects, list):
            continue
        for subject in subjects:
            if not isinstance(subject, dict):
                continue
            digest = subject.get("digest")
            if (
                subject.get("name") in expected_names
                and isinstance(digest, dict)
                and digest.get("sha256") == expected_sha
            ):
                return
    raise SystemExit("release attestation verifier JSON omitted material name/sha256 subject")


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


def verify_asset_index(
    index_path: Path,
    material: dict,
    *,
    dist_dir: Path,
    attestation_bundle_path: Path,
    metadata: dict,
) -> dict:
    index = read_json(index_path, "asset index")
    require_exact_fields(index, ASSET_INDEX_FIELDS, "asset index")
    require(index.get("schema_version") == SCHEMA_VERSION, "release integrity unsupported asset index schema_version")
    require(index.get("release_tag") == material["release_tag"], "release integrity asset index release_tag mismatch")
    require(isinstance(index.get("assets"), list), "release integrity asset index assets must be a list")
    require(index["assets"], "release integrity asset index assets must not be empty")
    seen: set[tuple[str, str, str]] = set()
    found_default = False
    for asset in index["assets"]:
        require(isinstance(asset, dict), "release integrity asset index asset must be an object")
        asset_name = asset.get("name") if isinstance(asset.get("name"), str) else "<unknown>"
        require_exact_fields(asset, ASSET_FIELDS, f"asset index asset {asset_name}")
        tuple_key = require_asset_tuple(asset)
        require(
            tuple_key not in seen,
            f"release integrity asset index duplicate tuple: {format_tuple(tuple_key)}",
        )
        seen.add(tuple_key)
        require(
            asset["release_tag"] == material["release_tag"],
            f"release integrity asset index release_tag mismatch for {asset_name}",
        )
        require(
            asset["m80_version"] == metadata["m80_version"],
            f"release integrity asset index m80_version mismatch for {asset_name}",
        )
        validate_asset_proof_refs(
            asset,
            dist_dir=dist_dir,
            attestation_bundle_name=attestation_bundle_path.name,
        )
        if tuple_key == ("linux", "x86_64", "minimal"):
            found_default = True
            verify_default_asset_row(asset, material=material, metadata=metadata, dist_dir=dist_dir)
    require(found_default, "release integrity asset index missing default bundle")
    return index


def require_asset_tuple(asset: dict) -> tuple[str, str, str]:
    key = (asset.get("os"), asset.get("arch"), asset.get("image_kind"))
    require(
        all(isinstance(value, str) and value for value in key),
        "release integrity asset index tuple fields invalid",
    )
    return key


def validate_asset_proof_refs(asset: dict, *, dist_dir: Path, attestation_bundle_name: str) -> None:
    asset_name = asset["name"]
    signature_name = asset["signature_name"]
    if signature_name is not None:
        signature_name = require_dist_asset_name(signature_name, "signature_name", asset_name)
        require(
            signature_name not in EXPECTED_SUBJECTS,
            f"release integrity asset index signature_name collides with required subject for {asset_name}: "
            f"{signature_name}",
        )
        require(
            (dist_dir / signature_name).is_file(),
            f"release integrity asset index signature_name missing file for {asset_name}: {signature_name}",
        )
    attestation_name = require_dist_asset_name(asset["attestation_name"], "attestation_name", asset_name)
    require(
        attestation_name == INTEGRITY_ATTESTATION_BUNDLE_NAME,
        "release integrity asset index attestation_name mismatch for "
        f"{asset_name}: expected {INTEGRITY_ATTESTATION_BUNDLE_NAME}, got {attestation_name}",
    )
    require(
        attestation_bundle_name == attestation_name,
        "release integrity attestation bundle path name mismatch: "
        f"expected {attestation_name}, got {attestation_bundle_name}",
    )
    require(
        (dist_dir / attestation_name).is_file(),
        f"release integrity asset index attestation_name missing file for {asset_name}: {attestation_name}",
    )


def require_dist_asset_name(value: object, field: str, asset_name: str) -> str:
    require(
        isinstance(value, str) and value,
        f"release integrity asset index {field} missing for {asset_name}",
    )
    require(
        "/" not in value and value not in {".", ".."} and DIST_ASSET_NAME_RE.match(value) is not None,
        f"release integrity asset index {field} invalid for {asset_name}",
    )
    return value


def verify_default_asset_row(asset: dict, *, material: dict, metadata: dict, dist_dir: Path) -> None:
    bundle_path = dist_dir / BUNDLE_NAME
    metadata_path = dist_dir / METADATA_NAME
    require(bundle_path.is_file(), f"release integrity default bundle missing: {BUNDLE_NAME}")
    require(metadata_path.is_file(), f"release integrity bundle metadata missing: {METADATA_NAME}")
    expected = {
        "name": BUNDLE_NAME,
        "url": release_asset_url(material["release_tag"], BUNDLE_NAME),
        "sha256": sha256_file(bundle_path),
        "size_bytes": bundle_path.stat().st_size,
        "metadata_name": METADATA_NAME,
        "metadata_sha256": sha256_file(metadata_path),
        "checksum_name": f"{BUNDLE_NAME}.sha256",
        "target": material["target"],
        "os": "linux",
        "arch": "x86_64",
        "image_kind": "minimal",
        "release_tag": material["release_tag"],
        "m80_version": metadata["m80_version"],
        "guest_protocol_version": metadata["guest_protocol_version"],
        "manifest_schema_version": metadata["manifest_schema_version"],
        "expected_firecracker_version": metadata["expected_firecracker_version"],
    }
    for field, expected_value in expected.items():
        require(
            asset[field] == expected_value,
            f"release integrity asset index {field} mismatch for {BUNDLE_NAME}",
        )


def verify_bootstrap_selector(selector_path: Path, material: dict, index: dict) -> None:
    require(selector_path.is_file(), f"bootstrap selector missing: {selector_path}")
    release_tag, actual_rows = parse_bootstrap_selector(selector_path)
    require(
        release_tag == material["release_tag"],
        "release integrity bootstrap selector release_tag mismatch",
    )
    require(
        release_tag == index["release_tag"],
        "release integrity bootstrap selector/index release_tag mismatch",
    )
    expected_rows = expected_bootstrap_selector_rows(index)
    missing = sorted(set(expected_rows) - set(actual_rows))
    extra = sorted(set(actual_rows) - set(expected_rows))
    require(
        not missing,
        "release integrity bootstrap selector missing tuple(s): " + ", ".join(format_tuple(row) for row in missing),
    )
    require(
        not extra,
        "release integrity bootstrap selector extra tuple(s): " + ", ".join(format_tuple(row) for row in extra),
    )
    for key, expected in expected_rows.items():
        actual = actual_rows[key]
        for field, expected_value, actual_value in zip(BOOTSTRAP_SELECTOR_COLUMNS, expected, actual, strict=True):
            require(
                actual_value == expected_value,
                f"release integrity bootstrap selector {field} mismatch for {format_tuple(key)}",
            )


def parse_bootstrap_selector(selector_path: Path) -> tuple[str, dict[tuple[str, str, str], list[str]]]:
    lines = selector_path.read_text().splitlines()
    require(len(lines) >= 3, "release integrity bootstrap selector header missing")
    require(
        lines[0].split("\t") == ["schema_version", str(BOOTSTRAP_SELECTOR_SCHEMA_VERSION)],
        "release integrity unsupported bootstrap selector schema_version",
    )
    release_parts = lines[1].split("\t")
    require(
        len(release_parts) == 2 and release_parts[0] == "release_tag",
        "release integrity bootstrap selector release_tag header invalid",
    )
    release_tag = release_parts[1]
    require(release_tag, "release integrity bootstrap selector release_tag missing")
    require(
        lines[2].split("\t") == ["columns", *BOOTSTRAP_SELECTOR_COLUMNS],
        "release integrity bootstrap selector columns mismatch",
    )
    rows: dict[tuple[str, str, str], list[str]] = {}
    for line in lines[3:]:
        parts = line.split("\t")
        require(
            len(parts) == len(BOOTSTRAP_SELECTOR_COLUMNS) + 1 and parts[0] == "row",
            "release integrity bootstrap selector row shape invalid",
        )
        values = parts[1:]
        for field, value in zip(BOOTSTRAP_SELECTOR_COLUMNS, values, strict=True):
            require(value, f"release integrity bootstrap selector {field} empty")
            require(
                not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in value),
                f"release integrity bootstrap selector {field} contains whitespace or control characters",
            )
            require(
                SELECTOR_VALUE_RE.match(value) is not None,
                f"release integrity bootstrap selector {field} contains non-shell-safe characters",
            )
        key = (values[0], values[1], values[2])
        require(key not in rows, f"release integrity bootstrap selector duplicate tuple: {format_tuple(key)}")
        rows[key] = values
    require(rows, "release integrity bootstrap selector missing tuple rows")
    return release_tag, rows


def expected_bootstrap_selector_rows(index: dict) -> dict[tuple[str, str, str], list[str]]:
    rows = {}
    for asset in index["assets"]:
        key = require_asset_tuple(asset)
        require(key not in rows, f"release integrity asset index duplicate bootstrap selector tuple: {format_tuple(key)}")
        rows[key] = [
            selector_value(asset.get("os"), "os"),
            selector_value(asset.get("arch"), "arch"),
            selector_value(asset.get("image_kind"), "image_kind"),
            selector_value(asset.get("name"), "name"),
            selector_value(asset.get("url"), "url"),
            selector_value(asset.get("sha256"), "sha256"),
            selector_value(asset.get("size_bytes"), "size_bytes"),
            selector_value(asset.get("metadata_name"), "metadata_name"),
            selector_value(asset.get("metadata_sha256"), "metadata_sha256"),
            selector_value(asset.get("checksum_name"), "checksum_name"),
            selector_value(asset.get("signature_name"), "signature_name"),
            selector_value(asset.get("attestation_name"), "attestation_name"),
            selector_value(asset.get("m80_version"), "m80_version"),
        ]
    return rows


def selector_value(value: object, field: str) -> str:
    if value is None:
        return "-"
    if isinstance(value, int):
        require(value > 0, f"release integrity bootstrap selector {field} must be greater than zero")
        return str(value)
    require(
        isinstance(value, str) and value,
        f"release integrity bootstrap selector {field} must not be empty",
    )
    require(
        not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in value),
        f"release integrity bootstrap selector {field} must not contain whitespace or control characters",
    )
    require(
        SELECTOR_VALUE_RE.match(value) is not None,
        f"release integrity bootstrap selector {field} must contain only shell-safe token characters",
    )
    return value


def format_tuple(key: tuple[str, str, str]) -> str:
    return "/".join(key)


def signature_subjects_from_index(index: dict) -> dict[str, str]:
    subjects = {}
    for asset in index["assets"]:
        signature_name = asset["signature_name"]
        if signature_name is not None:
            subjects[signature_name] = "detached-signature"
    return subjects


def verify_subjects(subjects: object, dist_dir: Path, extra_expected_subjects: dict[str, str]) -> None:
    require(isinstance(subjects, list), "release integrity subjects must be a list")
    expected_subjects = dict(EXPECTED_SUBJECTS)
    expected_subjects.update(extra_expected_subjects)
    by_name: dict[str, dict] = {}
    for subject in subjects:
        require(isinstance(subject, dict), "release integrity subject must be an object")
        name = require_subject_str(subject, "name", "<unknown>")
        require(name not in by_name, f"release integrity duplicate subject {name}")
        require_exact_fields(subject, SUBJECT_FIELDS, f"release integrity subject {name}")
        kind = require_subject_str(subject, "kind", name)
        expected_kind = expected_subjects.get(name)
        require(expected_kind is not None, f"release integrity unexpected subject {name}")
        require(kind == expected_kind, f"release integrity subject {name} kind mismatch")
        digest = require_subject_str(subject, "sha256", name)
        require_valid_sha(digest, f"release integrity subject {name} sha256")
        size_bytes = subject["size_bytes"]
        require(isinstance(size_bytes, int) and size_bytes > 0, f"release integrity subject {name} invalid size_bytes")
        by_name[name] = subject

    missing_subjects = sorted(set(expected_subjects) - set(by_name))
    require(not missing_subjects, "release integrity missing subject(s): " + ", ".join(missing_subjects))
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


def parse_timestamp(value: object, label: str) -> datetime:
    require(isinstance(value, str) and value, f"{label} missing")
    try:
        timestamp = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise SystemExit(f"{label} must be RFC3339") from exc
    require(timestamp.tzinfo is not None, f"{label} must include timezone")
    return timestamp.astimezone(timezone.utc)


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
