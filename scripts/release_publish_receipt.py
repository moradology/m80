#!/usr/bin/env python3
"""Write and verify the m80 release publish decision receipt."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re


SCHEMA_VERSION = 1
RECEIPT_NAME = "m80-release-publish-decision.json"
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
HOSTLESS_PROOF_NAME = "m80-quickstart-proof-hostless.json"
KIND = "m80_release_publish_decision"
DECISIONS = {"approved", "failed"}

TOP_LEVEL_FIELDS = {
    "schema_version",
    "kind",
    "decision",
    "release_tag",
    "commit_sha",
    "workflow_run_id",
    "workflow_run_attempt",
    "actor",
    "repository",
    "github_ref",
    "environment_approval_id",
    "generated_at",
    "artifact_manifest",
    "artifact_manifest_digest",
    "proof_ledger",
    "proof_ledger_digest",
    "public_assets",
    "failure_reason",
}
FILE_REF_FIELDS = {"name", "sha256", "size_bytes"}
PUBLIC_ASSET_FIELDS = {"name", "kind", "sha256", "size_bytes", "integrity_subject"}

RAW_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
DIST_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-dir", required=True, type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--workflow-run-id", required=True)
    parser.add_argument("--workflow-run-attempt", default="1")
    parser.add_argument("--actor", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--github-ref", required=True)
    parser.add_argument("--environment-approval-id", default=None)
    parser.add_argument("--generated-at", default=None)
    parser.add_argument("--decision", choices=sorted(DECISIONS), default="approved")
    parser.add_argument("--failure-reason", default=None)
    parser.add_argument(
        "--artifact-manifest",
        type=Path,
        help=f"default: <dist-dir>/{UPLOAD_MANIFEST_NAME}",
    )
    parser.add_argument(
        "--proof-ledger",
        type=Path,
        help=f"default: <dist-dir>/{HOSTLESS_PROOF_NAME}",
    )
    parser.add_argument(
        "--receipt",
        type=Path,
        help=f"default: <dist-dir>/{RECEIPT_NAME}",
    )
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dist_dir = args.dist_dir.resolve()
    manifest_path = (args.artifact_manifest or dist_dir / UPLOAD_MANIFEST_NAME).resolve()
    proof_path = (args.proof_ledger or dist_dir / HOSTLESS_PROOF_NAME).resolve()
    receipt_path = (args.receipt or dist_dir / RECEIPT_NAME).resolve()

    if args.write:
        receipt = build_receipt(
            dist_dir=dist_dir,
            release_tag=args.release_tag,
            commit_sha=args.commit_sha,
            workflow_run_id=args.workflow_run_id,
            workflow_run_attempt=args.workflow_run_attempt,
            actor=args.actor,
            repository=args.repository,
            github_ref=args.github_ref,
            environment_approval_id=args.environment_approval_id,
            generated_at=args.generated_at,
            decision=args.decision,
            failure_reason=args.failure_reason,
            manifest_path=manifest_path,
            proof_path=proof_path,
        )
        write_json(receipt_path, receipt)

    receipt = read_json(receipt_path, "release publish decision receipt")
    verify_receipt(
        receipt,
        dist_dir=dist_dir,
        release_tag=args.release_tag,
        commit_sha=args.commit_sha,
        workflow_run_id=args.workflow_run_id,
        workflow_run_attempt=args.workflow_run_attempt,
        actor=args.actor,
        repository=args.repository,
        github_ref=args.github_ref,
        manifest_path=manifest_path,
        proof_path=proof_path,
    )
    print(f"release publish decision receipt ok: {receipt_path}")
    return 0


def build_receipt(
    *,
    dist_dir: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
    workflow_run_attempt: str,
    actor: str,
    repository: str,
    github_ref: str,
    environment_approval_id: str | None,
    generated_at: str | None,
    decision: str,
    failure_reason: str | None,
    manifest_path: Path,
    proof_path: Path,
) -> dict:
    manifest = read_json(manifest_path, "release upload manifest")
    public_assets = normalized_public_assets(manifest)
    artifact_manifest = file_ref(dist_dir, manifest_path, "artifact manifest")
    proof_ledger = file_ref(dist_dir, proof_path, "proof ledger")
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "decision": decision,
        "release_tag": release_tag,
        "commit_sha": commit_sha,
        "workflow_run_id": workflow_run_id,
        "workflow_run_attempt": workflow_run_attempt,
        "actor": actor,
        "repository": repository,
        "github_ref": github_ref,
        "environment_approval_id": environment_approval_id,
        "generated_at": generated_at or utc_now(),
        "artifact_manifest": artifact_manifest,
        "artifact_manifest_digest": artifact_manifest["sha256"],
        "proof_ledger": proof_ledger,
        "proof_ledger_digest": proof_ledger["sha256"],
        "public_assets": public_assets,
        "failure_reason": failure_reason,
    }


def verify_receipt(
    receipt: dict,
    *,
    dist_dir: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
    workflow_run_attempt: str,
    actor: str,
    repository: str,
    github_ref: str,
    manifest_path: Path,
    proof_path: Path,
) -> None:
    require_exact_fields(receipt, TOP_LEVEL_FIELDS, "release publish decision receipt")
    require(receipt["schema_version"] == SCHEMA_VERSION, "release publish decision receipt schema_version mismatch")
    require(receipt["kind"] == KIND, "release publish decision receipt kind mismatch")
    require(receipt["decision"] in DECISIONS, "release publish decision receipt decision invalid")
    require(receipt["release_tag"] == release_tag, "release publish decision receipt release_tag mismatch")
    require(COMMIT_RE.fullmatch(commit_sha) is not None, "commit-sha must be a 40-character lowercase hex commit")
    require(receipt["commit_sha"] == commit_sha, "release publish decision receipt commit_sha mismatch")
    require_positive_int_string(workflow_run_id, "workflow-run-id")
    require(receipt["workflow_run_id"] == workflow_run_id, "release publish decision receipt workflow_run_id mismatch")
    require_positive_int_string(workflow_run_attempt, "workflow-run-attempt")
    require(
        receipt["workflow_run_attempt"] == workflow_run_attempt,
        "release publish decision receipt workflow_run_attempt mismatch",
    )
    require_safe_context_string(actor, "actor")
    require(receipt["actor"] == actor, "release publish decision receipt actor mismatch")
    require_repository(repository)
    require(receipt["repository"] == repository, "release publish decision receipt repository mismatch")
    require(
        github_ref == f"refs/tags/{release_tag}",
        "github-ref must be the release tag ref",
    )
    require(receipt["github_ref"] == github_ref, "release publish decision receipt github_ref mismatch")
    require(parse_timestamp(receipt["generated_at"]) is not None, "release publish decision receipt generated_at invalid")
    approval = receipt["environment_approval_id"]
    require(
        approval is None or (isinstance(approval, str) and approval.strip()),
        "release publish decision receipt environment_approval_id invalid",
    )
    failure_reason = receipt["failure_reason"]
    if receipt["decision"] == "approved":
        require(failure_reason is None, "approved publish decision receipt must not carry failure_reason")
    else:
        require(isinstance(failure_reason, str) and failure_reason.strip(), "failed publish decision requires failure_reason")

    artifact_manifest = verify_file_ref(
        receipt["artifact_manifest"],
        dist_dir=dist_dir,
        path=manifest_path,
        label="artifact manifest",
    )
    require(
        receipt["artifact_manifest_digest"] == artifact_manifest["sha256"],
        "release publish decision receipt artifact_manifest_digest mismatch",
    )
    proof_ledger = verify_file_ref(
        receipt["proof_ledger"],
        dist_dir=dist_dir,
        path=proof_path,
        label="proof ledger",
    )
    require(
        receipt["proof_ledger_digest"] == proof_ledger["sha256"],
        "release publish decision receipt proof_ledger_digest mismatch",
    )

    manifest = read_json(manifest_path, "release upload manifest")
    require(manifest.get("release_tag") == release_tag, "release upload manifest release_tag mismatch")
    expected_assets = normalized_public_assets(manifest)
    observed_assets = normalized_receipt_assets(receipt["public_assets"])
    require(
        observed_assets == expected_assets,
        "release publish decision receipt public_assets mismatch",
    )
    for asset in observed_assets:
        path = dist_dir / asset["name"]
        require(path.is_file(), f"release publish decision receipt public asset missing: {asset['name']}")
        require(asset["sha256"] == sha256_file(path), f"release publish decision receipt public asset {asset['name']} sha256 stale")
        require(asset["size_bytes"] == path.stat().st_size, f"release publish decision receipt public asset {asset['name']} size stale")


def normalized_public_assets(manifest: dict) -> list[dict]:
    public_assets = manifest.get("public_assets")
    require(isinstance(public_assets, list), "release upload manifest public_assets must be a list")
    result = [normalized_public_asset(row, "release upload manifest public asset") for row in public_assets]
    names = [row["name"] for row in result]
    require(len(names) == len(set(names)), "release upload manifest public asset duplicate name")
    return sorted(result, key=lambda row: row["name"])


def normalized_receipt_assets(value: object) -> list[dict]:
    require(isinstance(value, list), "release publish decision receipt public_assets must be a list")
    result = [normalized_public_asset(row, "release publish decision receipt public asset") for row in value]
    names = [row["name"] for row in result]
    require(len(names) == len(set(names)), "release publish decision receipt public asset duplicate name")
    return sorted(result, key=lambda row: row["name"])


def normalized_public_asset(row: object, label: str) -> dict:
    require(isinstance(row, dict), f"{label} must be an object")
    require_exact_fields(row, PUBLIC_ASSET_FIELDS, label)
    name = require_dist_name(row["name"], f"{label} name")
    require(isinstance(row["kind"], str) and row["kind"].strip(), f"{label} {name} kind invalid")
    require_raw_sha256(row["sha256"], f"{label} {name} sha256")
    require_non_negative_int(row["size_bytes"], f"{label} {name} size_bytes")
    require(isinstance(row["integrity_subject"], bool), f"{label} {name} integrity_subject invalid")
    return {
        "name": name,
        "kind": row["kind"],
        "sha256": row["sha256"],
        "size_bytes": row["size_bytes"],
        "integrity_subject": row["integrity_subject"],
    }


def file_ref(dist_dir: Path, path: Path, label: str) -> dict:
    require(path.is_file(), f"release publish {label} missing: {path}")
    name = require_dist_name(relative_dist_name(dist_dir, path), f"release publish {label} name")
    return {
        "name": name,
        "sha256": sha256_digest(path),
        "size_bytes": path.stat().st_size,
    }


def verify_file_ref(ref: object, *, dist_dir: Path, path: Path, label: str) -> dict:
    require(isinstance(ref, dict), f"release publish decision receipt {label} must be an object")
    require_exact_fields(ref, FILE_REF_FIELDS, f"release publish decision receipt {label}")
    expected = file_ref(dist_dir, path, label)
    require(ref["name"] == expected["name"], f"release publish decision receipt {label} name mismatch")
    require(ref["sha256"] == expected["sha256"], f"release publish decision receipt {label} sha256 mismatch")
    require(ref["size_bytes"] == expected["size_bytes"], f"release publish decision receipt {label} size_bytes mismatch")
    return expected


def relative_dist_name(dist_dir: Path, path: Path) -> str:
    try:
        rel = path.relative_to(dist_dir)
    except ValueError as exc:
        raise SystemExit(f"release publish input must be inside dist dir: {path}") from exc
    require(len(rel.parts) == 1, f"release publish input must be a top-level dist file: {path}")
    return rel.name


def require_dist_name(value: object, label: str) -> str:
    require(
        isinstance(value, str)
        and value
        and value not in {".", ".."}
        and "/" not in value
        and DIST_NAME_RE.fullmatch(value) is not None,
        f"{label} must be a flat dist file name",
    )
    return value


def require_raw_sha256(value: object, label: str) -> None:
    require(isinstance(value, str) and RAW_SHA256_RE.fullmatch(value) is not None, f"{label} must be lowercase sha256")


def require_non_negative_int(value: object, label: str) -> None:
    require(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")


def require_positive_int_string(value: object, label: str) -> None:
    require(isinstance(value, str) and value.isdigit() and int(value) > 0, f"{label} must be a positive integer")


def require_safe_context_string(value: object, label: str) -> None:
    require(
        isinstance(value, str)
        and value.strip()
        and all(ch >= " " and ch != "\x7f" for ch in value),
        f"{label} must be a nonempty printable string",
    )


def require_repository(value: object) -> None:
    require_safe_context_string(value, "repository")
    require(isinstance(value, str) and value.count("/") == 1, "repository must be owner/repo")


def require_exact_fields(obj: dict, expected: set[str], label: str) -> None:
    observed = set(obj)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} field mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def comma_or_none(values: list[str]) -> str:
    return ", ".join(values) if values else "none"


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


def parse_timestamp(value: object) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sha256_digest(path: Path) -> str:
    return f"sha256:{sha256_file(path)}"


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
