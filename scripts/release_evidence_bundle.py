#!/usr/bin/env python3
"""Write and verify the m80 release evidence bundle schema."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
from typing import Any


SCHEMA_VERSION = 2
KIND = "m80_release_evidence_bundle"
EVIDENCE_BUNDLE_NAME = "m80-release-evidence.json"
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
BUILD_HANDOFF_NAME = "m80-release-build.json"
PUBLISH_RECEIPT_NAME = "m80-release-publish-decision.json"
HOSTLESS_PROOF_NAME = "m80-quickstart-proof-hostless.json"
RELEASE_PROOF_LEDGER_NAME = "m80-release-proof-ledger.jsonl"
READINESS_CONFIG = Path("docs/behaviors/release/release-readiness-lanes.json")

TOP_LEVEL_FIELDS = {
    "schema_version",
    "kind",
    "release_tag",
    "commit_sha",
    "workflow_run_id",
    "m80_version",
    "resolved_install_tag",
    "generated_at",
    "required_lane_ids",
    "missing_required_lane_ids",
    "upload_manifest",
    "build_handoff",
    "publish_decision_receipt",
    "proof_ledger",
    "public_assets",
    "workflow_only_artifacts",
    "proofs",
    "redaction",
}
FILE_REF_FIELDS = {"name", "sha256", "size_bytes"}
PUBLIC_ASSET_FIELDS = {"name", "kind", "sha256", "size_bytes", "integrity_subject"}
WORKFLOW_ARTIFACT_FIELDS = {"name", "reason", "sha256", "size_bytes"}
PROOF_FIELDS = {"lane_id", "proof_kind", "substrate", "artifact_class", "file"}
REDACTION_FIELDS = {"policy", "forbidden"}
REDACTION_FORBIDDEN = ["absolute-host-paths", "secrets", "tokens", "environment-dumps"]

RAW_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
SHA256_REF_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
DIST_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")
LANE_ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
VERSION_TAG_RE = re.compile(r"^v[A-Za-z0-9][A-Za-z0-9._+-]*$")
PROOF_SUBSTRATES_BY_LANE = {
    "hostless-quickstart": "hostless",
    "real-kvm-quickstart": "real-kvm",
    "public-access-latest": "public-github",
    "latest-freshness": "public-github",
    "workflow-policy": "github-actions",
    "release-bundle-integrity": "github-actions",
}
PROOF_KINDS_BY_LANE = {
    "hostless-quickstart": "quickstart-proof",
    "real-kvm-quickstart": "quickstart-proof",
    "public-access-latest": "public-access-proof",
    "latest-freshness": "freshness-proof",
    "workflow-policy": "workflow-policy-report",
    "release-bundle-integrity": "release-integrity-predicate",
}
ARTIFACT_CLASSES = {"public", "workflow-only"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-dir", required=True, type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--workflow-run-id", required=True)
    parser.add_argument("--m80-version", required=True)
    parser.add_argument("--resolved-install-tag", default=None)
    parser.add_argument("--generated-at", default=None)
    parser.add_argument("--readiness-config", type=Path, default=READINESS_CONFIG)
    parser.add_argument("--upload-manifest", type=Path)
    parser.add_argument("--build-handoff", type=Path)
    parser.add_argument("--publish-receipt", type=Path)
    parser.add_argument("--proof-ledger", type=Path)
    parser.add_argument("--hostless-proof", type=Path)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dist_dir = args.dist_dir.resolve()
    upload_manifest = (args.upload_manifest or dist_dir / UPLOAD_MANIFEST_NAME).resolve()
    build_handoff = (args.build_handoff or dist_dir / BUILD_HANDOFF_NAME).resolve()
    publish_receipt = (args.publish_receipt or dist_dir / PUBLISH_RECEIPT_NAME).resolve()
    proof_ledger = (args.proof_ledger or dist_dir / RELEASE_PROOF_LEDGER_NAME).resolve()
    hostless_proof = (args.hostless_proof or dist_dir / HOSTLESS_PROOF_NAME).resolve()
    bundle_path = (args.bundle or dist_dir / EVIDENCE_BUNDLE_NAME).resolve()
    resolved_install_tag = args.resolved_install_tag or args.release_tag

    if args.write:
        bundle = build_bundle(
            dist_dir=dist_dir,
            release_tag=args.release_tag,
            commit_sha=args.commit_sha,
            workflow_run_id=args.workflow_run_id,
            m80_version=args.m80_version,
            resolved_install_tag=resolved_install_tag,
            generated_at=args.generated_at,
            readiness_config=args.readiness_config,
            upload_manifest=upload_manifest,
            build_handoff=build_handoff,
            publish_receipt=publish_receipt,
            proof_ledger=proof_ledger,
            hostless_proof=hostless_proof,
        )
        write_json(bundle_path, bundle)

    bundle = read_json(bundle_path, "release evidence bundle")
    verify_bundle(
        bundle,
        dist_dir=dist_dir,
        release_tag=args.release_tag,
        commit_sha=args.commit_sha,
        workflow_run_id=args.workflow_run_id,
        m80_version=args.m80_version,
        resolved_install_tag=resolved_install_tag,
        upload_manifest=upload_manifest,
        build_handoff=build_handoff,
        publish_receipt=publish_receipt,
        proof_ledger=proof_ledger,
        hostless_proof=hostless_proof,
    )
    print(f"release evidence bundle ok: {bundle_path}")
    return 0


def build_bundle(
    *,
    dist_dir: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
    m80_version: str,
    resolved_install_tag: str,
    generated_at: str | None,
    readiness_config: Path,
    upload_manifest: Path,
    build_handoff: Path,
    publish_receipt: Path,
    proof_ledger: Path,
    hostless_proof: Path,
) -> dict[str, Any]:
    manifest = read_json(upload_manifest, "release upload manifest")
    required_lane_ids = required_lanes(read_json(readiness_config, "release readiness config"))
    proof_lane_ids = {"hostless-quickstart"}
    workflow_only = workflow_only_artifacts(manifest, dist_dir)
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "release_tag": release_tag,
        "commit_sha": commit_sha,
        "workflow_run_id": workflow_run_id,
        "m80_version": m80_version,
        "resolved_install_tag": resolved_install_tag,
        "generated_at": generated_at or utc_now(),
        "required_lane_ids": required_lane_ids,
        "missing_required_lane_ids": sorted(set(required_lane_ids) - proof_lane_ids),
        "upload_manifest": file_ref(dist_dir, upload_manifest, "upload manifest"),
        "build_handoff": file_ref(dist_dir, build_handoff, "build handoff"),
        "publish_decision_receipt": file_ref(dist_dir, publish_receipt, "publish decision receipt"),
        "proof_ledger": file_ref(dist_dir, proof_ledger, "proof ledger"),
        "public_assets": normalized_public_assets(manifest.get("public_assets")),
        "workflow_only_artifacts": workflow_only,
        "proofs": [
            {
                "lane_id": "hostless-quickstart",
                "proof_kind": "quickstart-proof",
                "substrate": "hostless",
                "artifact_class": "workflow-only",
                "file": file_ref(dist_dir, hostless_proof, "hostless proof"),
            }
        ],
        "redaction": {
            "policy": "Evidence names flat release files and sha256 digests only; host paths, secrets, tokens, and environment dumps are forbidden.",
            "forbidden": list(REDACTION_FORBIDDEN),
        },
    }


def verify_bundle(
    bundle: dict[str, Any],
    *,
    dist_dir: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
    m80_version: str,
    resolved_install_tag: str,
    upload_manifest: Path,
    build_handoff: Path,
    publish_receipt: Path,
    proof_ledger: Path,
    hostless_proof: Path,
) -> None:
    require_exact_fields(bundle, TOP_LEVEL_FIELDS, "release evidence bundle")
    require(bundle["schema_version"] == SCHEMA_VERSION, "unsupported release evidence bundle schema_version")
    require(bundle["kind"] == KIND, "release evidence bundle kind mismatch")
    require(bundle["release_tag"] == release_tag, "release evidence bundle release_tag mismatch")
    require(COMMIT_RE.fullmatch(commit_sha) is not None, "commit-sha must be a 40-character lowercase hex commit")
    require(bundle["commit_sha"] == commit_sha, "release evidence bundle commit_sha mismatch")
    require_positive_int_string(workflow_run_id, "workflow-run-id")
    require(bundle["workflow_run_id"] == workflow_run_id, "release evidence bundle workflow_run_id mismatch")
    require_nonempty_string(bundle["m80_version"], "release evidence bundle m80_version")
    require(bundle["m80_version"] == m80_version, "release evidence bundle m80_version mismatch")
    require(VERSION_TAG_RE.fullmatch(resolved_install_tag) is not None, "resolved-install-tag must be a concrete v* tag")
    require(
        bundle["resolved_install_tag"] == resolved_install_tag,
        "release evidence bundle resolved_install_tag mismatch",
    )
    require(parse_timestamp(bundle["generated_at"]) is not None, "release evidence bundle generated_at invalid")

    upload_ref = verify_file_ref(bundle["upload_manifest"], dist_dir, upload_manifest, "upload manifest")
    build_ref = verify_file_ref(bundle["build_handoff"], dist_dir, build_handoff, "build handoff")
    receipt_ref = verify_file_ref(bundle["publish_decision_receipt"], dist_dir, publish_receipt, "publish decision receipt")
    proof_ref = verify_file_ref(bundle["proof_ledger"], dist_dir, proof_ledger, "proof ledger")
    del upload_ref, build_ref, receipt_ref, proof_ref

    manifest = read_json(upload_manifest, "release upload manifest")
    expected_public = normalized_public_assets(manifest.get("public_assets"))
    observed_public = normalized_public_assets(bundle["public_assets"])
    require(observed_public == expected_public, "release evidence bundle public_assets mismatch")

    observed_workflow = normalized_workflow_artifacts(bundle["workflow_only_artifacts"])

    public_names = {row["name"] for row in observed_public}
    workflow_names = {row["name"] for row in observed_workflow}
    overlap = sorted(public_names & workflow_names)
    require(not overlap, f"release evidence bundle public/workflow artifact confusion: {comma_or_none(overlap)}")

    expected_workflow = workflow_only_artifacts(manifest, dist_dir)
    require(observed_workflow == expected_workflow, "release evidence bundle workflow_only_artifacts mismatch")

    required_lane_ids = require_lane_ids(bundle["required_lane_ids"], "release evidence bundle required_lane_ids")
    require(set(required_lane_ids), "release evidence bundle required_lane_ids must be nonempty")
    missing_lane_ids = require_lane_ids(
        bundle["missing_required_lane_ids"],
        "release evidence bundle missing_required_lane_ids",
    )
    proof_lane_ids = require_proofs(
        bundle["proofs"],
        public_names=public_names,
        workflow_names=workflow_names,
        dist_dir=dist_dir,
        expected_files={"hostless-quickstart": hostless_proof},
    )
    required = set(required_lane_ids)
    missing = set(missing_lane_ids)
    require(missing <= required, "release evidence bundle missing lane is not required")
    require(proof_lane_ids <= required, "release evidence bundle proof lane is not required")
    require(
        not (missing & proof_lane_ids),
        "release evidence bundle lane cannot be both missing and proven",
    )
    require(
        missing | proof_lane_ids == required,
        "release evidence bundle required lane coverage mismatch",
    )
    verify_redaction(bundle["redaction"])


def normalized_public_assets(value: object) -> list[dict[str, Any]]:
    rows = require_list(value, "release evidence bundle public_assets")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release evidence bundle public asset must be an object")
        require_exact_fields(row, PUBLIC_ASSET_FIELDS, "release evidence bundle public asset")
        name = require_dist_name(row["name"], "release evidence bundle public asset name")
        require_nonempty_string(row["kind"], f"release evidence bundle public asset {name} kind")
        require_raw_sha256(row["sha256"], f"release evidence bundle public asset {name} sha256")
        require_non_negative_int(row["size_bytes"], f"release evidence bundle public asset {name} size_bytes")
        require(isinstance(row["integrity_subject"], bool), f"release evidence bundle public asset {name} integrity_subject invalid")
        result.append(
            {
                "name": name,
                "kind": row["kind"],
                "sha256": row["sha256"],
                "size_bytes": row["size_bytes"],
                "integrity_subject": row["integrity_subject"],
            }
        )
    names = [row["name"] for row in result]
    require(len(names) == len(set(names)), "release evidence bundle public asset duplicate name")
    return sorted(result, key=lambda row: row["name"])


def workflow_only_artifacts(manifest: dict[str, Any], dist_dir: Path) -> list[dict[str, Any]]:
    rows = require_list(manifest.get("non_public_workflow_artifacts"), "release upload manifest non_public_workflow_artifacts")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release upload manifest non-public workflow artifact must be an object")
        require_exact_fields(row, {"name", "reason"}, "release upload manifest non-public workflow artifact")
        name = require_dist_name(row["name"], "release upload manifest non-public workflow artifact name")
        require_nonempty_string(row["reason"], f"release upload manifest non-public workflow artifact {name} reason")
        path = dist_dir / name
        require(path.is_file(), f"release evidence workflow-only artifact missing: {name}")
        result.append(
            {
                "name": name,
                "reason": row["reason"],
                "sha256": sha256_file(path),
                "size_bytes": path.stat().st_size,
            }
        )
    names = [row["name"] for row in result]
    require(len(names) == len(set(names)), "release evidence workflow-only artifact duplicate name")
    return sorted(result, key=lambda row: row["name"])


def normalized_workflow_artifacts(value: object) -> list[dict[str, Any]]:
    rows = require_list(value, "release evidence bundle workflow_only_artifacts")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release evidence bundle workflow-only artifact must be an object")
        require_exact_fields(row, WORKFLOW_ARTIFACT_FIELDS, "release evidence bundle workflow-only artifact")
        name = require_dist_name(row["name"], "release evidence bundle workflow-only artifact name")
        require_nonempty_string(row["reason"], f"release evidence bundle workflow-only artifact {name} reason")
        require_raw_sha256(row["sha256"], f"release evidence bundle workflow-only artifact {name} sha256")
        require_non_negative_int(row["size_bytes"], f"release evidence bundle workflow-only artifact {name} size_bytes")
        result.append(
            {
                "name": name,
                "reason": row["reason"],
                "sha256": row["sha256"],
                "size_bytes": row["size_bytes"],
            }
        )
    names = [row["name"] for row in result]
    require(len(names) == len(set(names)), "release evidence bundle workflow-only artifact duplicate name")
    return sorted(result, key=lambda row: row["name"])


def require_proofs(
    value: object,
    *,
    public_names: set[str],
    workflow_names: set[str],
    dist_dir: Path,
    expected_files: dict[str, Path],
) -> set[str]:
    proofs = require_list(value, "release evidence bundle proofs")
    seen_lane_ids: set[str] = set()
    seen_file_names: set[str] = set()
    for proof in proofs:
        require(isinstance(proof, dict), "release evidence bundle proof must be an object")
        require_exact_fields(proof, PROOF_FIELDS, "release evidence bundle proof")
        lane_id = require_lane_id(proof["lane_id"], "release evidence bundle proof lane_id")
        require(lane_id not in seen_lane_ids, f"release evidence bundle duplicate proof lane_id: {lane_id}")
        seen_lane_ids.add(lane_id)
        expected_substrate = PROOF_SUBSTRATES_BY_LANE.get(lane_id)
        if expected_substrate is not None:
            require(proof["substrate"] == expected_substrate, f"release evidence bundle proof {lane_id} substrate mismatch")
        expected_kind = PROOF_KINDS_BY_LANE.get(lane_id)
        if expected_kind is not None:
            require(proof["proof_kind"] == expected_kind, f"release evidence bundle proof {lane_id} proof_kind mismatch")
        require(proof["artifact_class"] in ARTIFACT_CLASSES, f"release evidence bundle proof {lane_id} artifact_class invalid")
        file = normalized_file_ref(proof["file"], f"release evidence bundle proof {lane_id} file")
        if proof["artifact_class"] == "public":
            require(file["name"] in public_names, f"release evidence bundle proof {lane_id} public artifact not listed as public")
        else:
            require(file["name"] in workflow_names, f"release evidence bundle proof {lane_id} workflow artifact not listed as workflow-only")
        require(
            file["name"] not in seen_file_names,
            f"release evidence bundle duplicate proof file: {file['name']}",
        )
        seen_file_names.add(file["name"])
        expected_file = expected_files.get(lane_id)
        if expected_file is not None:
            expected_ref = file_ref(dist_dir, expected_file, f"proof {lane_id}")
            require(file == expected_ref, f"release evidence bundle proof {lane_id} file mismatch")
    return seen_lane_ids


def verify_redaction(value: object) -> None:
    require(isinstance(value, dict), "release evidence bundle redaction must be an object")
    require_exact_fields(value, REDACTION_FIELDS, "release evidence bundle redaction")
    require_nonempty_string(value["policy"], "release evidence bundle redaction policy")
    forbidden = require_string_list(value["forbidden"], "release evidence bundle redaction forbidden")
    require(set(forbidden) == set(REDACTION_FORBIDDEN), "release evidence bundle redaction forbidden set mismatch")


def required_lanes(config: dict[str, Any]) -> list[str]:
    lanes = require_list(config.get("lanes"), "release readiness config lanes")
    result = []
    for lane in lanes:
        require(isinstance(lane, dict), "release readiness config lane must be an object")
        if lane.get("severity") != "required":
            continue
        result.append(require_lane_id(lane.get("id"), "release readiness config lane id"))
    require(result, "release readiness config must contain required lanes")
    require(len(result) == len(set(result)), "release readiness config duplicate required lane id")
    return sorted(result)


def require_lane_ids(value: object, label: str) -> list[str]:
    lane_ids = [require_lane_id(row, label) for row in require_list(value, label)]
    require(len(lane_ids) == len(set(lane_ids)), f"{label} duplicate lane id")
    return lane_ids


def require_lane_id(value: object, label: str) -> str:
    require(isinstance(value, str) and LANE_ID_RE.fullmatch(value) is not None, f"{label} must be kebab-case")
    return value


def file_ref(dist_dir: Path, path: Path, label: str) -> dict[str, Any]:
    require(path.is_file(), f"release evidence {label} missing: {path}")
    name = require_dist_name(relative_dist_name(dist_dir, path), f"release evidence {label} name")
    return {"name": name, "sha256": f"sha256:{sha256_file(path)}", "size_bytes": path.stat().st_size}


def verify_file_ref(value: object, dist_dir: Path, path: Path, label: str) -> dict[str, Any]:
    observed = normalized_file_ref(value, f"release evidence bundle {label}")
    expected = file_ref(dist_dir, path, label)
    require(observed == expected, f"release evidence bundle {label} mismatch")
    return expected


def normalized_file_ref(value: object, label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, FILE_REF_FIELDS, label)
    name = require_dist_name(value["name"], f"{label} name")
    require_sha256_ref(value["sha256"], f"{label} sha256")
    require_non_negative_int(value["size_bytes"], f"{label} size_bytes")
    return {"name": name, "sha256": value["sha256"], "size_bytes": value["size_bytes"]}


def relative_dist_name(dist_dir: Path, path: Path) -> str:
    try:
        rel = path.relative_to(dist_dir)
    except ValueError as exc:
        raise SystemExit(f"release evidence input must be inside dist dir: {path}") from exc
    require(len(rel.parts) == 1, f"release evidence input must be a top-level dist file: {path}")
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


def require_sha256_ref(value: object, label: str) -> None:
    require(isinstance(value, str) and SHA256_REF_RE.fullmatch(value) is not None, f"{label} must be sha256:<lowercase digest>")


def require_nonempty_string(value: object, label: str) -> str:
    require(isinstance(value, str) and value.strip(), f"{label} must be a nonempty string")
    return value


def require_string_list(value: object, label: str) -> list[str]:
    rows = require_list(value, label)
    result = []
    for row in rows:
        result.append(require_nonempty_string(row, f"{label} item"))
    require(len(result) == len(set(result)), f"{label} duplicate item")
    return result


def require_list(value: object, label: str) -> list[object]:
    require(isinstance(value, list), f"{label} must be a list")
    return value


def require_non_negative_int(value: object, label: str) -> None:
    require(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")


def require_positive_int_string(value: object, label: str) -> None:
    require(isinstance(value, str) and value.isdigit() and int(value) > 0, f"{label} must be a positive integer")


def require_exact_fields(obj: dict[str, Any], expected: set[str], label: str) -> None:
    observed = set(obj)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} field mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def comma_or_none(values: list[str]) -> str:
    return ", ".join(values) if values else "none"


def read_json(path: Path, label: str) -> dict[str, Any]:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        payload = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{label} is not valid JSON: {exc}") from exc
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def write_json(path: Path, payload: dict[str, Any]) -> None:
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


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require(condition: object, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
