#!/usr/bin/env python3
"""Write normalized m80 release attestation metadata from a verified bundle."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess


SCHEMA_VERSION = 1
MECHANISM = "github-artifact-attestation"
REPOSITORY = "moradology/m80"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--material", required=True, type=Path)
    parser.add_argument("--attestation-bundle", required=True, type=Path)
    parser.add_argument("--trust-policy", required=True, type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--gh-bin", default="gh")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    require(args.material.is_file(), f"release integrity material missing: {args.material}")
    require(args.attestation_bundle.is_file(), f"release attestation bundle missing: {args.attestation_bundle}")
    require(COMMIT_RE.match(args.commit_sha) is not None, "commit sha must be a 40-character lowercase hex digest")
    policy = read_json(args.trust_policy, "release trust policy")
    validate_policy(policy)
    material_sha = sha256_file(args.material)
    valid_from = format_timestamp(parse_timestamp(policy["valid_from"], "release trust valid_from"))
    valid_until = format_timestamp(parse_timestamp(policy["valid_until"], "release trust valid_until"))
    require(valid_from <= valid_until, "release trust validity window is inverted")
    signer_identity, signer_issuer = verify_attestation(
        gh_bin=args.gh_bin,
        material=args.material,
        attestation_bundle=args.attestation_bundle,
        signers=allowed_signers(policy),
        release_tag=args.release_tag,
        commit_sha=args.commit_sha,
        material_sha=material_sha,
    )
    payload = {
        "schema_version": SCHEMA_VERSION,
        "mechanism": MECHANISM,
        "repository": REPOSITORY,
        "release_tag": args.release_tag,
        "predicate_sha256": material_sha,
        "signer_identity": signer_identity,
        "issuer": signer_issuer,
        "keyset_id": nonempty_str(policy, "keyset_id", "release trust policy"),
        "certificate_not_before": valid_from,
        "certificate_not_after": valid_until,
    }
    args.out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    args.out.chmod(0o644)
    print(args.out)
    return 0


def validate_policy(policy: dict) -> None:
    require_exact_fields(policy, TRUST_POLICY_FIELDS, "release trust policy")
    require(policy["schema_version"] == SCHEMA_VERSION, "release trust policy schema_version mismatch")
    require(policy["mechanism"] == MECHANISM, "release trust policy mechanism mismatch")
    require(policy["repository"] == REPOSITORY, "release trust policy repository mismatch")
    require(nonempty_str(policy, "keyset_id", "release trust policy"), "release trust keyset_id missing")
    parse_timestamp(policy["valid_from"], "release trust valid_from")
    parse_timestamp(policy["valid_until"], "release trust valid_until")


def allowed_signers(policy: dict) -> list[tuple[str, str]]:
    rows = policy["allowed_signers"]
    require(isinstance(rows, list) and rows, "release trust allowed_signers must not be empty")
    signers = []
    for row in rows:
        require(isinstance(row, dict), "release trust allowed signer must be an object")
        require_exact_fields(row, TRUST_SIGNER_FIELDS, "release trust allowed signer")
        signers.append(
            (
                nonempty_str(row, "identity", "release trust allowed signer"),
                nonempty_str(row, "issuer", "release trust allowed signer"),
            )
        )
    return signers


def verify_attestation(
    *,
    gh_bin: str,
    material: Path,
    attestation_bundle: Path,
    signers: list[tuple[str, str]],
    release_tag: str,
    commit_sha: str,
    material_sha: str,
) -> tuple[str, str]:
    failures = []
    for signer_identity, signer_issuer in signers:
        cmd = [
            gh_bin,
            "attestation",
            "verify",
            str(material),
            "--repo",
            REPOSITORY,
            "--bundle",
            str(attestation_bundle),
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
            failures.append(f"{signer_identity} / {signer_issuer}")
            continue
        verified = parse_verified_output(completed.stdout)
        require_material_subject(verified, material, material_sha)
        return signer_identity, signer_issuer
    raise SystemExit(
        "release attestation metadata could not verify allowed signer(s): "
        + ", ".join(failures)
    )


def parse_verified_output(stdout: str) -> list:
    require(stdout.strip(), "release attestation verifier returned empty JSON")
    try:
        verified = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise SystemExit("release attestation verifier returned invalid JSON") from exc
    require(isinstance(verified, list) and verified, "release attestation verifier returned no attestations")
    return verified


def require_material_subject(verified: list, material: Path, material_sha: str) -> None:
    expected_names = {str(material), material.name}
    for entry in verified:
        if not isinstance(entry, dict):
            continue
        result = entry.get("verificationResult")
        if not isinstance(result, dict):
            continue
        statement = result.get("statement")
        if not isinstance(statement, dict):
            continue
        for subject in statement.get("subject", []):
            if not isinstance(subject, dict):
                continue
            digest = subject.get("digest")
            if (
                subject.get("name") in expected_names
                and isinstance(digest, dict)
                and digest.get("sha256") == material_sha
            ):
                return
    raise SystemExit("release attestation verifier JSON omitted material name/sha256 subject")


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


def nonempty_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} missing {key}")
    return value


def parse_timestamp(value: object, label: str) -> datetime:
    require(isinstance(value, str) and value, f"{label} missing")
    try:
        timestamp = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise SystemExit(f"{label} must be RFC3339") from exc
    require(timestamp.tzinfo is not None, f"{label} must include timezone")
    return timestamp.astimezone(timezone.utc)


def format_timestamp(value: datetime) -> str:
    return value.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


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
