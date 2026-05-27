#!/usr/bin/env python3
"""Verify a downloaded release install.sh before privileged execution."""

from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import shlex
import sys


SCRIPT_DIR = Path(__file__).resolve().parent
INTEGRITY_MODULE_PATH = SCRIPT_DIR / "verify-release-integrity.py"
INSTALL_NAME = "install.sh"
INTEGRITY_NAME = "m80-release-integrity.json"
ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"
ATTESTATION_METADATA_NAME = "m80-release-attestation.json"
VERIFIED_ASSET_NAMES = (
    INSTALL_NAME,
    INTEGRITY_NAME,
    ATTESTATION_BUNDLE_NAME,
    ATTESTATION_METADATA_NAME,
)


def load_integrity_module():
    spec = importlib.util.spec_from_file_location("m80_verify_release_integrity", INTEGRITY_MODULE_PATH)
    if spec is None or spec.loader is None:
        raise SystemExit(f"unable to load release integrity verifier: {INTEGRITY_MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


verify = load_integrity_module()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dist_dir", type=Path, help="directory containing the downloaded install handoff assets")
    parser.add_argument("--release-tag", required=True, help="expected concrete release tag, for example v1.2.3")
    parser.add_argument("--trust-policy", required=True, type=Path, help="trusted release trust policy")
    parser.add_argument("--attestation-bundle", type=Path, help=f"default: <dist-dir>/{ATTESTATION_BUNDLE_NAME}")
    parser.add_argument("--attestation-metadata", type=Path, help=f"default: <dist-dir>/{ATTESTATION_METADATA_NAME}")
    parser.add_argument("--verification-time", required=True, help="RFC3339 verification time")
    parser.add_argument("--gh-bin", default="gh", help=argparse.SUPPRESS)
    parser.add_argument("--json", action="store_true", help="render machine-readable handoff evidence")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return verify_install_handoff(args)
    except SystemExit as exc:
        if isinstance(exc.code, str):
            print(exc.code, file=sys.stderr)
            print(handoff_failure_remediation(args.release_tag), file=sys.stderr)
            return 1
        raise


def verify_install_handoff(args: argparse.Namespace) -> int:
    dist_dir = args.dist_dir
    verify.require(dist_dir.is_dir(), f"install handoff dist dir missing: {dist_dir}")
    material_path = dist_dir / INTEGRITY_NAME
    install_path = dist_dir / INSTALL_NAME
    attestation_bundle_path = args.attestation_bundle or dist_dir / ATTESTATION_BUNDLE_NAME
    attestation_metadata_path = args.attestation_metadata or dist_dir / ATTESTATION_METADATA_NAME

    material = read_material(material_path, args.release_tag)
    commit_sha = material["commit_sha"]
    verification_time = verify.parse_timestamp(args.verification_time, "verification time")
    verify.verify_trust_anchor(
        material,
        material_path=material_path,
        trust_policy_path=args.trust_policy,
        attestation_bundle_path=attestation_bundle_path,
        attestation_path=attestation_metadata_path,
        release_tag=args.release_tag,
        commit_sha=commit_sha,
        verification_time=verification_time,
    )
    install_sha256 = verify_install_script_subjects(material, install_path)
    payload = {
        "verified_install_handoff": True,
        "release_tag": args.release_tag,
        "commit_sha": commit_sha,
        "install_sh_sha256": install_sha256,
        "verified_assets": list(VERIFIED_ASSET_NAMES),
        "sudo_command": f"sudo sh {shlex.quote(str(install_path))}",
    }
    if args.json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print(f"verified install handoff: release_tag={args.release_tag} commit={commit_sha}")
        print(f"install_sh_sha256={install_sha256}")
        print(f"verified_assets={','.join(VERIFIED_ASSET_NAMES)}")
        print(payload["sudo_command"])
    return 0


def handoff_failure_remediation(release_tag: str) -> str:
    return (
        "repair: retry the pinned installer "
        f"`curl -fsSL https://github.com/{verify.REPOSITORY}/releases/download/{release_tag}/install.sh | sudo sh` "
        "or inspect the release asset list for missing or stale files; do not fetch installer scripts from mutable branches."
    )


def read_material(path: Path, release_tag: str) -> dict:
    material = verify.read_json(path, "release integrity material")
    verify.require_exact_fields(material, verify.TOP_LEVEL_FIELDS, "release integrity material")
    verify.require(
        material["schema_version"] == verify.SCHEMA_VERSION,
        f"unsupported release integrity schema_version: expected {verify.SCHEMA_VERSION}, got {material['schema_version']}",
    )
    verify.require(
        material["mechanism"] == verify.MECHANISM,
        f"release integrity mechanism mismatch: expected {verify.MECHANISM}, got {material['mechanism']}",
    )
    verify.require(
        material["repository"] == verify.REPOSITORY,
        f"release integrity repository mismatch: expected {verify.REPOSITORY}, got {material['repository']}",
    )
    verify.require(
        material["release_tag"] == release_tag,
        f"release integrity release_tag mismatch: expected {release_tag}, got {material['release_tag']}",
    )
    commit_sha = material["commit_sha"]
    verify.require(
        isinstance(commit_sha, str) and verify.COMMIT_RE.match(commit_sha) is not None,
        "release integrity commit_sha invalid",
    )
    verify.require_nonempty_str(material, "rust_toolchain", "release integrity material")
    verify.require_nonempty_str(material, "m80_package_version", "release integrity material")
    verify.require_nonempty_str(material, "bundle_metadata_name", "release integrity material")
    verify.require_valid_sha(material["bundle_metadata_sha256"], "release integrity bundle_metadata_sha256")
    return material


def verify_install_script_subjects(material: dict, install_path: Path) -> str:
    verify.require(install_path.is_file(), f"install script missing: {install_path}")
    actual_install_digest = verify.sha256_file(install_path)
    by_name = subject_map(material)
    verify_named_subject(
        by_name,
        INSTALL_NAME,
        "installer",
        install_path,
        expected_digest=actual_install_digest,
    )
    return actual_install_digest


def subject_map(material: dict) -> dict[str, dict]:
    subjects = material.get("subjects")
    verify.require(isinstance(subjects, list), "release integrity subjects must be a list")
    by_name: dict[str, dict] = {}
    for subject in subjects:
        verify.require(isinstance(subject, dict), "release integrity subject must be an object")
        name = verify.require_subject_str(subject, "name", "<unknown>")
        verify.require(name not in by_name, f"release integrity duplicate subject {name}")
        verify.require_exact_fields(subject, verify.SUBJECT_FIELDS, f"release integrity subject {name}")
        verify.require_subject_str(subject, "kind", name)
        verify.require_valid_sha(subject["sha256"], f"release integrity subject {name} sha256")
        verify.require(
            isinstance(subject["size_bytes"], int) and subject["size_bytes"] > 0,
            f"release integrity subject {name} invalid size_bytes",
        )
        by_name[name] = subject
    return by_name


def verify_named_subject(
    by_name: dict[str, dict],
    name: str,
    expected_kind: str,
    path: Path,
    *,
    expected_digest: str | None = None,
) -> None:
    subject = by_name.get(name)
    verify.require(subject is not None, f"release integrity missing subject(s): {name}")
    verify.require(subject["kind"] == expected_kind, f"release integrity subject {name} kind mismatch")
    verify.require(path.is_file(), f"release integrity subject file missing: {name}")
    actual_digest = verify.sha256_file(path)
    verify.require(
        subject["sha256"] == actual_digest,
        f"release integrity sha256 mismatch for {name}: expected {subject['sha256']}, got {actual_digest}",
    )
    if expected_digest is not None:
        verify.require(
            subject["sha256"] == expected_digest,
            f"release integrity install.sh subject digest mismatch: expected {expected_digest}, got {subject['sha256']}",
        )
    actual_size = path.stat().st_size
    verify.require(
        subject["size_bytes"] == actual_size,
        f"release integrity size mismatch for {name}: expected {subject['size_bytes']}, got {actual_size}",
    )


if __name__ == "__main__":
    raise SystemExit(main())
