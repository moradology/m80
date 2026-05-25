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
from release_common import parse_timestamp, require, sha256_file


SCHEMA_VERSION = 6
KIND = "m80_release_evidence_bundle"
EVIDENCE_BUNDLE_NAME = "m80-release-evidence.json"
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
BUILD_HANDOFF_NAME = "m80-release-build.json"
PUBLISH_RECEIPT_NAME = "m80-release-publish-decision.json"
READINESS_DECISION_NAME = "m80-release-readiness-decision.json"
TOKEN_AUTHORITY_NAME = "m80-release-token-authority.json"
REMOTE_INVENTORY_NAME = "m80-release-remote-assets.json"
PUBLIC_ACCESS_RECEIPT_NAME = "release-readiness-public-access.json"
HOSTLESS_PROOF_NAME = "m80-quickstart-proof-hostless.json"
REAL_KVM_PROOF_NAME = "m80-quickstart-proof-real-kvm.json"
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
    "receipt_refs",
    "public_assets",
    "workflow_only_artifacts",
    "proofs",
    "host_binaries",
    "release_identity",
    "substrate_policy",
    "redaction",
}
FILE_REF_FIELDS = {"name", "sha256", "size_bytes"}
PUBLIC_ASSET_FIELDS = {"name", "kind", "sha256", "size_bytes", "integrity_subject"}
WORKFLOW_ARTIFACT_FIELDS = {"name", "reason", "sha256", "size_bytes"}
WORKFLOW_INVENTORY_FIELDS = {"name", "reason", "sha256", "size_bytes"}
RECEIPT_REF_FIELDS = {
    "id",
    "artifact_class",
    "file",
    "schema_version",
    "kind",
    "release_tag",
    "commit_sha",
    "workflow_run_id",
}
PROOF_FIELDS = {"lane_id", "proof_kind", "substrate", "artifact_class", "file"}
HOST_BINARIES_FIELDS = {
    "lane_id",
    "substrate",
    "artifact_class",
    "manifest",
    "firecracker_version",
    "jailer_version",
    "install_root_classification",
}
RELEASE_IDENTITY_FIELDS = {
    "lane_id",
    "substrate",
    "requested_tag",
    "resolved_install_tag",
    "m80_version",
    "m80_release_tag",
    "bundle_release_tag",
    "bundle_m80_version",
    "manifest_schema_version",
    "guest_protocol_version",
    "bundle_metadata",
}
SUBSTRATE_POLICY_FIELDS = {
    "lane_id",
    "proof_kind",
    "observed_proof_kind",
    "observed_substrate",
    "required_substrates",
    "required_substrate_class",
    "proof_fixture",
    "publish_blocking",
    "may_satisfy_publish",
    "may_satisfy_latest",
    "proof",
    "config",
}
REDACTION_FIELDS = {"policy", "forbidden"}
REDACTION_FORBIDDEN = ["absolute-host-paths", "secrets", "tokens", "environment-dumps"]

RAW_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
SHA256_REF_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
DIST_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")
LANE_ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
DIAGNOSTIC_VALUE_LIMIT = 160
SECRET_VALUE_RE = re.compile(
    r"github_pat_[A-Za-z0-9_]+"
    r"|gh[pousr]_[A-Za-z0-9_]{16,}"
    r"|(?i:(token|secret|password|authorization)[=:][^\s,;]+)"
    r"|-----BEGIN [A-Z ]*PRIVATE KEY-----"
)
ABSOLUTE_HOST_PATH_RE = re.compile(
    r"(?<![A-Za-z0-9_.-])/(?:home|root|tmp|var|run|workspace|tank|Users|opt)(?:/[^\s,;:\"']*)?"
)
VERSION_TAG_RE = re.compile(r"^v[A-Za-z0-9][A-Za-z0-9._+-]*$")
PROOF_SUBSTRATES_BY_LANE = {
    "hostless-quickstart": "hostless",
    "real-kvm-quickstart": "real-kvm",
    "public-access-latest": "public-github",
    "latest-freshness": "public-github",
    "workflow-policy": "github-actions",
    "release-bundle-integrity": "github-actions",
}
PROOF_PAYLOAD_KINDS_BY_LANE = {
    "hostless-quickstart": "hostless",
    "real-kvm-quickstart": "real-kvm",
}
REQUIRED_SUBSTRATE_CLASS_BY_LANE = {
    "hostless-quickstart": "fixture",
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
RECEIPT_DEFINITIONS = {
    "publish-decision": {
        "name": PUBLISH_RECEIPT_NAME,
        "kind": "m80_release_publish_decision",
        "schema_version": 4,
        "artifact_class": "workflow-only",
        "commit_field": "commit_sha",
        "workflow_field": "workflow_run_id",
    },
    "readiness-decision": {
        "name": READINESS_DECISION_NAME,
        "kind": "m80_release_readiness_decision",
        "schema_version": 1,
        "artifact_class": "workflow-only",
        "commit_field": "commit_sha",
        "workflow_field": "workflow_run_id",
    },
    "token-authority": {
        "name": TOKEN_AUTHORITY_NAME,
        "kind": "m80_release_publish_token_authority",
        "schema_version": 1,
        "artifact_class": "workflow-only",
        "commit_field": "commit_sha",
        "workflow_field": "workflow_run_id",
    },
    "remote-asset-inventory": {
        "name": REMOTE_INVENTORY_NAME,
        "kind": "m80_release_remote_asset_inventory",
        "schema_version": 1,
        "artifact_class": "workflow-only",
        "commit_field": None,
        "workflow_field": None,
    },
    "public-access": {
        "name": PUBLIC_ACCESS_RECEIPT_NAME,
        "kind": "m80_release_readiness_public_access",
        "schema_version": 1,
        "artifact_class": "workflow-only",
        "commit_field": "commit_sha",
        "workflow_field": None,
    },
}


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
    parser.add_argument("--real-kvm-proof", type=Path)
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
    real_kvm_proof = args.real_kvm_proof.resolve() if args.real_kvm_proof else None
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
            real_kvm_proof=real_kvm_proof,
        )
        write_json(bundle_path, bundle)

    verify_bundle_path(bundle_path, dist_dir)
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
        readiness_config=args.readiness_config,
        hostless_proof=hostless_proof,
        real_kvm_proof=real_kvm_proof,
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
    real_kvm_proof: Path | None,
) -> dict[str, Any]:
    manifest = read_json(upload_manifest, "release upload manifest")
    readiness = read_json(readiness_config, "release readiness config")
    lane_policy_by_id = readiness_lanes_by_id(readiness)
    latest_lane_ids = latest_stage_lane_ids(readiness)
    required_lane_ids = required_lanes(readiness)
    proof_inputs = {"hostless-quickstart": hostless_proof}
    if real_kvm_proof is not None:
        proof_inputs["real-kvm-quickstart"] = real_kvm_proof
    workflow_only = workflow_only_artifacts(manifest, dist_dir)
    workflow_by_name = {row["name"]: row for row in workflow_only}
    require(
        real_kvm_proof is None or real_kvm_proof.name in workflow_by_name,
        "release evidence real-kvm proof must be listed as a workflow-only artifact",
    )
    proof_lane_ids = set(proof_inputs)
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
        "receipt_refs": receipt_ref_rows(
            dist_dir=dist_dir,
            release_tag=release_tag,
            commit_sha=commit_sha,
            workflow_run_id=workflow_run_id,
            present_paths=present_receipt_paths(dist_dir, publish_receipt),
        ),
        "public_assets": normalized_public_assets(manifest.get("public_assets")),
        "workflow_only_artifacts": workflow_only,
        "proofs": [
            {
                "lane_id": lane_id,
                "proof_kind": "quickstart-proof",
                "substrate": PROOF_SUBSTRATES_BY_LANE[lane_id],
                "artifact_class": "workflow-only",
                "file": file_ref(dist_dir, proof_path, f"{lane_id} proof"),
            }
            for lane_id, proof_path in sorted(proof_inputs.items())
            if lane_id == "hostless-quickstart" or proof_path.name in workflow_by_name
        ],
        "host_binaries": host_binaries_summaries(
            dist_dir=dist_dir,
            proof_inputs=proof_inputs,
            workflow_by_name=workflow_by_name,
            lane_policy_by_id=lane_policy_by_id,
            config_path=readiness_config,
        ),
        "release_identity": release_identity_summaries(
            dist_dir=dist_dir,
            proof_inputs=proof_inputs,
            release_tag=release_tag,
            resolved_install_tag=resolved_install_tag,
            m80_version=m80_version,
        ),
        "substrate_policy": substrate_policy_summaries(
            dist_dir=dist_dir,
            proof_inputs=proof_inputs,
            lane_policy_by_id=lane_policy_by_id,
            latest_lane_ids=latest_lane_ids,
            config_path=readiness_config,
        ),
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
    readiness_config: Path,
    hostless_proof: Path,
    real_kvm_proof: Path | None,
) -> None:
    require_exact_fields(bundle, TOP_LEVEL_FIELDS, "release evidence bundle")
    require(
        bundle["schema_version"] == SCHEMA_VERSION,
        diagnostic_mismatch(
            "schema_version",
            EVIDENCE_BUNDLE_NAME,
            SCHEMA_VERSION,
            bundle["schema_version"],
            "rerun scripts/release_evidence_bundle.py --write with the current verifier",
        ),
    )
    require(
        bundle["kind"] == KIND,
        diagnostic_mismatch(
            "kind",
            EVIDENCE_BUNDLE_NAME,
            KIND,
            bundle["kind"],
            "rerun scripts/release_evidence_bundle.py --write",
        ),
    )
    require(
        bundle["release_tag"] == release_tag,
        diagnostic_mismatch(
            "release_tag",
            EVIDENCE_BUNDLE_NAME,
            release_tag,
            bundle["release_tag"],
            "rerun scripts/release_evidence_bundle.py --write with the release tag being published",
        ),
    )
    require(COMMIT_RE.fullmatch(commit_sha) is not None, "commit-sha must be a 40-character lowercase hex commit")
    require(
        bundle["commit_sha"] == commit_sha,
        diagnostic_mismatch(
            "commit_sha",
            EVIDENCE_BUNDLE_NAME,
            commit_sha,
            bundle["commit_sha"],
            "rerun scripts/release_evidence_bundle.py --write for the source commit being published",
        ),
    )
    require_positive_int_string(workflow_run_id, "workflow-run-id")
    require(
        bundle["workflow_run_id"] == workflow_run_id,
        diagnostic_mismatch(
            "workflow_run_id",
            EVIDENCE_BUNDLE_NAME,
            workflow_run_id,
            bundle["workflow_run_id"],
            "rerun scripts/release_evidence_bundle.py --write in the current release workflow run",
        ),
    )
    require_nonempty_string(bundle["m80_version"], "release evidence bundle m80_version")
    require(
        bundle["m80_version"] == m80_version,
        diagnostic_mismatch(
            "m80_version",
            EVIDENCE_BUNDLE_NAME,
            m80_version,
            bundle["m80_version"],
            "rebuild the release bundle, then rerun scripts/release_evidence_bundle.py --write",
        ),
    )
    require(VERSION_TAG_RE.fullmatch(resolved_install_tag) is not None, "resolved-install-tag must be a concrete v* tag")
    require(
        bundle["resolved_install_tag"] == resolved_install_tag,
        diagnostic_mismatch(
            "resolved_install_tag",
            EVIDENCE_BUNDLE_NAME,
            resolved_install_tag,
            bundle["resolved_install_tag"],
            "resolve latest once, then rerun scripts/release_evidence_bundle.py --write",
        ),
    )
    require(parse_timestamp(bundle["generated_at"]) is not None, "release evidence bundle generated_at invalid")

    upload_ref = verify_file_ref(
        bundle["upload_manifest"],
        dist_dir,
        upload_manifest,
        field_path="upload_manifest",
        label="upload manifest",
        repair_command="rerun scripts/release_evidence_bundle.py --write after regenerating the upload manifest",
    )
    build_ref = verify_file_ref(
        bundle["build_handoff"],
        dist_dir,
        build_handoff,
        field_path="build_handoff",
        label="build handoff",
        repair_command="rerun scripts/package-release-bundle.py then scripts/release_evidence_bundle.py --write",
    )
    receipt_ref = verify_file_ref(
        bundle["publish_decision_receipt"],
        dist_dir,
        publish_receipt,
        field_path="publish_decision_receipt",
        label="publish decision receipt",
        repair_command="rerun scripts/release_publish_receipt.py --write then scripts/release_evidence_bundle.py --write",
    )
    proof_ref = verify_file_ref(
        bundle["proof_ledger"],
        dist_dir,
        proof_ledger,
        field_path="proof_ledger",
        label="proof ledger",
        repair_command="rerun the release proof ledger producer then scripts/release_evidence_bundle.py --write",
    )
    del upload_ref, build_ref, receipt_ref, proof_ref

    manifest = read_json(upload_manifest, "release upload manifest")
    expected_public = normalized_public_assets(manifest.get("public_assets"))
    observed_public = normalized_public_assets(bundle["public_assets"])
    require(
        observed_public == expected_public,
        diagnostic_mismatch(
            "public_assets",
            EVIDENCE_BUNDLE_NAME,
            expected_public,
            observed_public,
            "rerun scripts/release_upload_manifest.py --write, then scripts/release_evidence_bundle.py --write",
        ),
    )

    observed_workflow = normalized_workflow_artifacts(bundle["workflow_only_artifacts"])
    expected_receipts = receipt_ref_rows(
        dist_dir=dist_dir,
        release_tag=release_tag,
        commit_sha=commit_sha,
        workflow_run_id=workflow_run_id,
        present_paths=present_receipt_paths(dist_dir, publish_receipt),
    )
    observed_receipts = normalized_receipt_refs(
        bundle["receipt_refs"],
        dist_dir=dist_dir,
        release_tag=release_tag,
        commit_sha=commit_sha,
        workflow_run_id=workflow_run_id,
    )
    require(
        observed_receipts == expected_receipts,
        diagnostic_mismatch(
            "receipt_refs",
            EVIDENCE_BUNDLE_NAME,
            expected_receipts,
            observed_receipts,
            "rerun the receipt producers then scripts/release_evidence_bundle.py --write",
        ),
    )

    public_names = {row["name"] for row in observed_public}
    workflow_names = {row["name"] for row in observed_workflow}
    overlap = sorted(public_names & workflow_names)
    require(not overlap, f"release evidence bundle public/workflow artifact confusion: {comma_or_none(overlap)}")

    expected_workflow = workflow_only_artifacts(manifest, dist_dir)
    require(
        observed_workflow == expected_workflow,
        diagnostic_mismatch(
            "workflow_only_artifacts",
            EVIDENCE_BUNDLE_NAME,
            expected_workflow,
            observed_workflow,
            "rerun scripts/release_upload_manifest.py --write after regenerating workflow-only artifacts",
        ),
    )
    workflow_by_name = {row["name"]: row for row in expected_workflow}
    readiness = read_json(readiness_config, "release readiness config")
    lane_policy_by_id = readiness_lanes_by_id(readiness)
    latest_lane_ids = latest_stage_lane_ids(readiness)

    required_lane_ids = require_lane_ids(bundle["required_lane_ids"], "release evidence bundle required_lane_ids")
    require(set(required_lane_ids), "release evidence bundle required_lane_ids must be nonempty")
    missing_lane_ids = require_lane_ids(
        bundle["missing_required_lane_ids"],
        "release evidence bundle missing_required_lane_ids",
    )
    expected_proof_files = {"hostless-quickstart": hostless_proof}
    if real_kvm_proof is not None:
        expected_proof_files["real-kvm-quickstart"] = real_kvm_proof
    proof_lane_ids = require_proofs(
        bundle["proofs"],
        public_names=public_names,
        workflow_names=workflow_names,
        dist_dir=dist_dir,
        expected_files=expected_proof_files,
    )
    proof_inputs = {"hostless-quickstart": hostless_proof}
    if real_kvm_proof is not None:
        proof_inputs["real-kvm-quickstart"] = real_kvm_proof
    expected_host_binaries = host_binaries_summaries(
        dist_dir=dist_dir,
        proof_inputs=proof_inputs,
        workflow_by_name=workflow_by_name,
        lane_policy_by_id=lane_policy_by_id,
        config_path=readiness_config,
    )
    observed_host_binaries = normalized_host_binaries(bundle["host_binaries"])
    require(
        observed_host_binaries == expected_host_binaries,
        diagnostic_mismatch(
            "host_binaries",
            EVIDENCE_BUNDLE_NAME,
            expected_host_binaries,
            observed_host_binaries,
            "rerun the quickstart proof lane, then scripts/release_evidence_bundle.py --write",
        ),
    )
    expected_release_identity = release_identity_summaries(
        dist_dir=dist_dir,
        proof_inputs=proof_inputs,
        release_tag=release_tag,
        resolved_install_tag=resolved_install_tag,
        m80_version=m80_version,
    )
    observed_release_identity = normalized_release_identity(bundle["release_identity"])
    require(
        observed_release_identity == expected_release_identity,
        diagnostic_mismatch(
            "release_identity",
            EVIDENCE_BUNDLE_NAME,
            expected_release_identity,
            observed_release_identity,
            "rerun the quickstart proof lane or rebuild the release bundle, then scripts/release_evidence_bundle.py --write",
        ),
    )
    expected_substrate_policy = substrate_policy_summaries(
        dist_dir=dist_dir,
        proof_inputs=proof_inputs,
        lane_policy_by_id=lane_policy_by_id,
        latest_lane_ids=latest_lane_ids,
        config_path=readiness_config,
    )
    observed_substrate_policy = normalized_substrate_policy(bundle["substrate_policy"])
    require(
        observed_substrate_policy == expected_substrate_policy,
        diagnostic_mismatch(
            "substrate_policy",
            EVIDENCE_BUNDLE_NAME,
            expected_substrate_policy,
            observed_substrate_policy,
            "rerun the proof lane on the configured substrate, then scripts/release_evidence_bundle.py --write",
        ),
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
    reject_absolute_host_path_leak(bundle, "release evidence bundle")


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
    rows = require_list(manifest.get("workflow_artifact_inventory"), "release upload manifest workflow_artifact_inventory")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release upload manifest workflow artifact inventory must be an object")
        require_exact_fields(row, WORKFLOW_INVENTORY_FIELDS, "release upload manifest workflow artifact inventory")
        name = require_dist_name(row["name"], "release upload manifest workflow artifact inventory name")
        require_nonempty_string(row["reason"], f"release upload manifest workflow artifact inventory {name} reason")
        require_raw_sha256(row["sha256"], f"release upload manifest workflow artifact inventory {name} sha256")
        require_non_negative_int(row["size_bytes"], f"release upload manifest workflow artifact inventory {name} size_bytes")
        path = dist_dir / name
        require(
            path.is_file(),
            diagnostic_mismatch(
                f"workflow_only_artifacts[{name}].file",
                name,
                "present file",
                "missing",
                "regenerate the workflow-only artifact, then rerun scripts/release_upload_manifest.py --write and scripts/release_evidence_bundle.py --write",
            ),
        )
        actual_sha = sha256_file(path)
        actual_size = path.stat().st_size
        require(
            row["sha256"] == actual_sha,
            diagnostic_mismatch(
                f"workflow_only_artifacts[{name}].sha256",
                name,
                actual_sha,
                row["sha256"],
                "rerun scripts/release_upload_manifest.py --write, then scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            row["size_bytes"] == actual_size,
            diagnostic_mismatch(
                f"workflow_only_artifacts[{name}].size_bytes",
                name,
                actual_size,
                row["size_bytes"],
                "rerun scripts/release_upload_manifest.py --write, then scripts/release_evidence_bundle.py --write",
            ),
        )
        result.append(
            {
                "name": name,
                "reason": row["reason"],
                "sha256": actual_sha,
                "size_bytes": actual_size,
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


def present_receipt_paths(dist_dir: Path, publish_receipt: Path) -> dict[str, Path]:
    paths = {"publish-decision": publish_receipt}
    for receipt_id, definition in RECEIPT_DEFINITIONS.items():
        if receipt_id == "publish-decision":
            continue
        path = dist_dir / definition["name"]
        if path.is_file():
            paths[receipt_id] = path
    return paths


def receipt_ref_rows(
    *,
    dist_dir: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
    present_paths: dict[str, Path],
) -> list[dict[str, Any]]:
    rows = []
    for receipt_id, path in sorted(present_paths.items()):
        definition = receipt_definition(receipt_id)
        payload = read_json(path, f"{receipt_id} receipt")
        validate_receipt_payload(
            receipt_id=receipt_id,
            payload=payload,
            release_tag=release_tag,
            commit_sha=commit_sha,
            workflow_run_id=workflow_run_id,
        )
        rows.append(
            {
                "id": receipt_id,
                "artifact_class": definition["artifact_class"],
                "file": file_ref(dist_dir, path, f"{receipt_id} receipt"),
                "schema_version": definition["schema_version"],
                "kind": definition["kind"],
                "release_tag": release_tag,
                "commit_sha": commit_sha,
                "workflow_run_id": workflow_run_id,
            }
        )
    return rows


def normalized_receipt_refs(
    value: object,
    *,
    dist_dir: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
) -> list[dict[str, Any]]:
    rows = require_list(value, "release evidence bundle receipt_refs")
    result = []
    seen_ids: set[str] = set()
    seen_files: set[str] = set()
    for row in rows:
        require(isinstance(row, dict), "release evidence bundle receipt_ref must be an object")
        require_exact_fields(row, RECEIPT_REF_FIELDS, "release evidence bundle receipt_ref")
        receipt_id = require_receipt_id(row["id"], "release evidence bundle receipt_ref id")
        require(receipt_id not in seen_ids, f"release evidence bundle duplicate receipt_ref id: {receipt_id}")
        seen_ids.add(receipt_id)
        definition = receipt_definition(receipt_id)
        require(
            row["artifact_class"] == definition["artifact_class"],
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].artifact_class",
                EVIDENCE_BUNDLE_NAME,
                definition["artifact_class"],
                row["artifact_class"],
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            row["schema_version"] == definition["schema_version"],
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].schema_version",
                EVIDENCE_BUNDLE_NAME,
                definition["schema_version"],
                row["schema_version"],
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            row["kind"] == definition["kind"],
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].kind",
                EVIDENCE_BUNDLE_NAME,
                definition["kind"],
                row["kind"],
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            row["release_tag"] == release_tag,
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].release_tag",
                EVIDENCE_BUNDLE_NAME,
                release_tag,
                row["release_tag"],
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            row["commit_sha"] == commit_sha,
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].commit_sha",
                EVIDENCE_BUNDLE_NAME,
                commit_sha,
                row["commit_sha"],
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            row["workflow_run_id"] == workflow_run_id,
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].workflow_run_id",
                EVIDENCE_BUNDLE_NAME,
                workflow_run_id,
                row["workflow_run_id"],
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
        expected_path = dist_dir / definition["name"]
        file = verify_file_ref(
            row["file"],
            dist_dir,
            expected_path,
            field_path=f"receipt_refs[{receipt_id}].file",
            label=f"{receipt_id} receipt",
            repair_command="rerun the receipt producer then scripts/release_evidence_bundle.py --write",
        )
        require(
            file["name"] not in seen_files,
            f"release evidence bundle duplicate receipt_ref file: {file['name']}",
        )
        seen_files.add(file["name"])
        payload = read_json(expected_path, f"{receipt_id} receipt")
        validate_receipt_payload(
            receipt_id=receipt_id,
            payload=payload,
            release_tag=release_tag,
            commit_sha=commit_sha,
            workflow_run_id=workflow_run_id,
        )
        result.append(
            {
                "id": receipt_id,
                "artifact_class": row["artifact_class"],
                "file": file,
                "schema_version": row["schema_version"],
                "kind": row["kind"],
                "release_tag": row["release_tag"],
                "commit_sha": row["commit_sha"],
                "workflow_run_id": row["workflow_run_id"],
            }
        )
    return sorted(result, key=lambda row: row["id"])


def validate_receipt_payload(
    *,
    receipt_id: str,
    payload: dict[str, Any],
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str,
) -> None:
    definition = receipt_definition(receipt_id)
    require(
        payload.get("schema_version") == definition["schema_version"],
        diagnostic_mismatch(
            f"receipt_refs[{receipt_id}].payload.schema_version",
            definition["name"],
            definition["schema_version"],
            payload.get("schema_version"),
            "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
        ),
    )
    require(
        payload.get("kind") == definition["kind"],
        diagnostic_mismatch(
            f"receipt_refs[{receipt_id}].payload.kind",
            definition["name"],
            definition["kind"],
            payload.get("kind"),
            "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
        ),
    )
    require(
        payload.get("release_tag") == release_tag,
        diagnostic_mismatch(
            f"receipt_refs[{receipt_id}].payload.release_tag",
            definition["name"],
            release_tag,
            payload.get("release_tag"),
            "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
        ),
    )
    commit_field = definition["commit_field"]
    if commit_field is not None:
        require(
            payload.get(commit_field) == commit_sha,
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].payload.{commit_field}",
                definition["name"],
                commit_sha,
                payload.get(commit_field),
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )
    workflow_field = definition["workflow_field"]
    if workflow_field is not None:
        require(
            payload.get(workflow_field) == workflow_run_id,
            diagnostic_mismatch(
                f"receipt_refs[{receipt_id}].payload.{workflow_field}",
                definition["name"],
                workflow_run_id,
                payload.get(workflow_field),
                "rerun the receipt producer then scripts/release_evidence_bundle.py --write",
            ),
        )


def receipt_definition(receipt_id: str) -> dict[str, Any]:
    definition = RECEIPT_DEFINITIONS.get(receipt_id)
    require(definition is not None, f"release evidence bundle unknown receipt_ref id: {receipt_id}")
    return definition


def require_receipt_id(value: object, label: str) -> str:
    require(isinstance(value, str) and value in RECEIPT_DEFINITIONS, f"{label} unknown")
    return value


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
            require(
                proof["substrate"] == expected_substrate,
                diagnostic_mismatch(
                    f"proofs[{lane_id}].substrate",
                    EVIDENCE_BUNDLE_NAME,
                    expected_substrate,
                    proof["substrate"],
                    "rerun the proof lane on the expected substrate, then scripts/release_evidence_bundle.py --write",
                ),
            )
        expected_kind = PROOF_KINDS_BY_LANE.get(lane_id)
        if expected_kind is not None:
            require(
                proof["proof_kind"] == expected_kind,
                diagnostic_mismatch(
                    f"proofs[{lane_id}].proof_kind",
                    EVIDENCE_BUNDLE_NAME,
                    expected_kind,
                    proof["proof_kind"],
                    "rerun the proof lane, then scripts/release_evidence_bundle.py --write",
                ),
            )
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
            expected_ref = verify_file_ref(
                proof["file"],
                dist_dir,
                expected_file,
                field_path=f"proofs[{lane_id}].file",
                label=f"proof {lane_id}",
                repair_command="rerun the quickstart proof lane then scripts/release_evidence_bundle.py --write",
            )
            del expected_ref
    return seen_lane_ids


def host_binaries_summaries(
    *,
    dist_dir: Path,
    proof_inputs: dict[str, Path],
    workflow_by_name: dict[str, dict[str, Any]],
    lane_policy_by_id: dict[str, dict[str, Any]],
    config_path: Path,
) -> list[dict[str, Any]]:
    rows = []
    for lane_id, proof_path in sorted(proof_inputs.items()):
        proof = read_json(proof_path, f"{lane_id} quickstart proof")
        validate_quickstart_proof_policy(
            lane_id=lane_id,
            proof=proof,
            proof_path=proof_path,
            lane_policy_by_id=lane_policy_by_id,
            config_path=config_path,
        )
        require(
            proof.get("proof_kind") == PROOF_PAYLOAD_KINDS_BY_LANE[lane_id],
            f"{lane_id} quickstart proof proof_kind mismatch",
        )
        host_binaries = require_object(proof.get("host_binaries"), f"{lane_id} quickstart proof host_binaries")
        require_exact_fields(
            host_binaries,
            {"manifest_path", "firecracker_version", "jailer_version"},
            f"{lane_id} quickstart proof host_binaries",
        )
        manifest_rel = require_relative_artifact_path(
            host_binaries["manifest_path"],
            f"{lane_id} quickstart proof host_binaries manifest_path",
        )
        require(len(Path(manifest_rel).parts) == 1, f"{lane_id} host-binaries manifest must be a flat artifact")
        manifest_path = (proof_path.parent / manifest_rel).resolve()
        manifest_ref = file_ref(dist_dir, manifest_path, f"{lane_id} host-binaries manifest")
        inventory_row = workflow_by_name.get(manifest_ref["name"])
        require(
            inventory_row is not None,
            f"release evidence host-binaries manifest not listed as workflow-only artifact: {manifest_ref['name']}",
        )
        require(
            manifest_ref["sha256"] == f"sha256:{inventory_row['sha256']}",
            diagnostic_mismatch(
                f"host_binaries[{lane_id}].manifest.sha256",
                manifest_ref["name"],
                f"sha256:{inventory_row['sha256']}",
                manifest_ref["sha256"],
                "rerun the quickstart proof lane, then scripts/release_upload_manifest.py --write and scripts/release_evidence_bundle.py --write",
            ),
        )
        require(
            manifest_ref["size_bytes"] == inventory_row["size_bytes"],
            diagnostic_mismatch(
                f"host_binaries[{lane_id}].manifest.size_bytes",
                manifest_ref["name"],
                inventory_row["size_bytes"],
                manifest_ref["size_bytes"],
                "rerun the quickstart proof lane, then scripts/release_upload_manifest.py --write and scripts/release_evidence_bundle.py --write",
            ),
        )
        manifest = read_json(manifest_path, f"{lane_id} host-binaries manifest")
        firecracker_version = require_nonempty_string(
            host_binaries["firecracker_version"],
            f"{lane_id} quickstart proof host_binaries firecracker_version",
        )
        jailer_version = require_nonempty_string(
            host_binaries["jailer_version"],
            f"{lane_id} quickstart proof host_binaries jailer_version",
        )
        require(
            manifest.get("firecracker_version") == firecracker_version,
            diagnostic_mismatch(
                f"host_binaries[{lane_id}].manifest.firecracker_version",
                manifest_ref["name"],
                firecracker_version,
                manifest.get("firecracker_version"),
                "rerun the quickstart proof lane so proof and host-binaries manifest agree",
            ),
        )
        require(
            manifest.get("jailer_version") == jailer_version,
            diagnostic_mismatch(
                f"host_binaries[{lane_id}].manifest.jailer_version",
                manifest_ref["name"],
                jailer_version,
                manifest.get("jailer_version"),
                "rerun the quickstart proof lane so proof and host-binaries manifest agree",
            ),
        )
        rows.append(
            {
                "lane_id": lane_id,
                "substrate": PROOF_SUBSTRATES_BY_LANE[lane_id],
                "artifact_class": "workflow-only",
                "manifest": manifest_ref,
                "firecracker_version": firecracker_version,
                "jailer_version": jailer_version,
                "install_root_classification": install_root_classification(proof),
            }
        )
    return rows


def normalized_host_binaries(value: object) -> list[dict[str, Any]]:
    rows = require_list(value, "release evidence bundle host_binaries")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release evidence bundle host_binaries row must be an object")
        require_exact_fields(row, HOST_BINARIES_FIELDS, "release evidence bundle host_binaries row")
        lane_id = require_lane_id(row["lane_id"], "release evidence bundle host_binaries lane_id")
        expected_substrate = PROOF_SUBSTRATES_BY_LANE.get(lane_id)
        require(expected_substrate is not None, f"release evidence bundle host_binaries {lane_id} lane unknown")
        require(row["substrate"] == expected_substrate, f"release evidence bundle host_binaries {lane_id} substrate mismatch")
        require(row["artifact_class"] == "workflow-only", f"release evidence bundle host_binaries {lane_id} artifact_class invalid")
        firecracker_version = require_nonempty_string(
            row["firecracker_version"],
            f"release evidence bundle host_binaries {lane_id} firecracker_version",
        )
        jailer_version = require_nonempty_string(
            row["jailer_version"],
            f"release evidence bundle host_binaries {lane_id} jailer_version",
        )
        classification = require_nonempty_string(
            row["install_root_classification"],
            f"release evidence bundle host_binaries {lane_id} install_root_classification",
        )
        require(
            classification in {"default-install-root", "override-install-root", "hostless-fixture"},
            f"release evidence bundle host_binaries {lane_id} install_root_classification invalid",
        )
        result.append(
            {
                "lane_id": lane_id,
                "substrate": row["substrate"],
                "artifact_class": row["artifact_class"],
                "manifest": normalized_file_ref(
                    row["manifest"],
                    f"release evidence bundle host_binaries {lane_id} manifest",
                ),
                "firecracker_version": firecracker_version,
                "jailer_version": jailer_version,
                "install_root_classification": classification,
            }
        )
    lane_ids = [row["lane_id"] for row in result]
    require(len(lane_ids) == len(set(lane_ids)), "release evidence bundle host_binaries duplicate lane_id")
    return sorted(result, key=lambda row: row["lane_id"])


def release_identity_summaries(
    *,
    dist_dir: Path,
    proof_inputs: dict[str, Path],
    release_tag: str,
    resolved_install_tag: str,
    m80_version: str,
) -> list[dict[str, Any]]:
    rows = []
    for lane_id, proof_path in sorted(proof_inputs.items()):
        proof = read_json(proof_path, f"{lane_id} quickstart proof")
        release = require_object(proof.get("release"), f"{lane_id} quickstart proof release")
        require_exact_fields(
            release,
            {"requested", "resolved_tag", "install_url"},
            f"{lane_id} quickstart proof release",
        )
        requested_tag = require_nonempty_string(release["requested"], f"{lane_id} quickstart proof release requested")
        proof_resolved_tag = require_nonempty_string(
            release["resolved_tag"],
            f"{lane_id} quickstart proof release resolved_tag",
        )
        require(
            proof_resolved_tag == resolved_install_tag,
            f"{lane_id} resolved install tag mismatch: expected {resolved_install_tag}, got {proof_resolved_tag}; source={proof_path.name}; repair=rerun quickstart proof for {resolved_install_tag}",
        )
        require(
            requested_tag == "latest" or requested_tag == proof_resolved_tag,
            f"{lane_id} requested tag mismatch: expected latest or {proof_resolved_tag}, got {requested_tag}; source={proof_path.name}; repair=rerun quickstart proof",
        )
        require(
            proof_resolved_tag != "latest",
            f"{lane_id} resolved install tag must be concrete, got latest; source={proof_path.name}; repair=resolve latest once before proof",
        )

        m80 = require_object(proof.get("m80"), f"{lane_id} quickstart proof m80")
        require_exact_fields(
            m80,
            {"version", "release_tag", "version_status"},
            f"{lane_id} quickstart proof m80",
        )
        proof_m80_version = require_nonempty_string(m80["version"], f"{lane_id} quickstart proof m80 version")
        proof_m80_release_tag = require_nonempty_string(
            m80["release_tag"],
            f"{lane_id} quickstart proof m80 release_tag",
        )
        require(
            proof_m80_version == m80_version,
            f"{lane_id} m80 version mismatch: expected {m80_version}, got {proof_m80_version}; source={proof_path.name}; repair=rebuild release bundle",
        )
        require(
            proof_m80_release_tag == release_tag,
            f"{lane_id} m80 release_tag mismatch: expected {release_tag}, got {proof_m80_release_tag}; source={proof_path.name}; repair=rebuild release bundle",
        )
        require(
            m80["version_status"] == "release",
            f"{lane_id} m80 version_status mismatch: expected release, got {m80['version_status']}; source={proof_path.name}; repair=use release binary",
        )

        bundle = require_object(proof.get("bundle"), f"{lane_id} quickstart proof bundle")
        require_exact_fields(
            bundle,
            {"metadata_path", "release_tag", "m80_version", "guest_protocol_version", "manifest_schema_version"},
            f"{lane_id} quickstart proof bundle",
        )
        metadata_rel = require_relative_artifact_path(
            bundle["metadata_path"],
            f"{lane_id} quickstart proof bundle metadata_path",
        )
        require(len(Path(metadata_rel).parts) == 1, f"{lane_id} bundle metadata must be a flat artifact")
        metadata_path = (proof_path.parent / metadata_rel).resolve()
        metadata_ref = file_ref(dist_dir, metadata_path, f"{lane_id} bundle metadata")
        metadata = read_json(metadata_path, f"{lane_id} bundle metadata")
        bundle_release_tag = require_nonempty_string(
            bundle["release_tag"],
            f"{lane_id} quickstart proof bundle release_tag",
        )
        bundle_m80_version = require_nonempty_string(
            bundle["m80_version"],
            f"{lane_id} quickstart proof bundle m80_version",
        )
        require(
            bundle_release_tag == release_tag,
            f"{lane_id} bundle release_tag mismatch: expected {release_tag}, got {bundle_release_tag}; source={proof_path.name}; repair=rebuild release bundle",
        )
        require(
            bundle_m80_version == m80_version,
            f"{lane_id} bundle m80_version mismatch: expected {m80_version}, got {bundle_m80_version}; source={proof_path.name}; repair=rebuild release bundle",
        )
        manifest_schema_version = require_positive_int(
            bundle["manifest_schema_version"],
            f"{lane_id} quickstart proof bundle manifest_schema_version",
        )
        guest_protocol_version = require_positive_int(
            bundle["guest_protocol_version"],
            f"{lane_id} quickstart proof bundle guest_protocol_version",
        )
        require(
            metadata.get("release_tag") == bundle_release_tag,
            f"{lane_id} bundle metadata release_tag mismatch: expected {bundle_release_tag}, got {metadata.get('release_tag')}; source={metadata_ref['name']}; repair=rebuild release bundle",
        )
        require(
            metadata.get("m80_version") == bundle_m80_version,
            f"{lane_id} bundle metadata m80_version mismatch: expected {bundle_m80_version}, got {metadata.get('m80_version')}; source={metadata_ref['name']}; repair=rebuild release bundle",
        )
        require(
            metadata.get("manifest_schema_version") == manifest_schema_version,
            f"{lane_id} bundle metadata manifest_schema_version mismatch: expected {manifest_schema_version}, got {metadata.get('manifest_schema_version')}; source={metadata_ref['name']}; repair=rebuild release bundle",
        )
        require(
            metadata.get("guest_protocol_version") == guest_protocol_version,
            f"{lane_id} bundle metadata guest_protocol_version mismatch: expected {guest_protocol_version}, got {metadata.get('guest_protocol_version')}; source={metadata_ref['name']}; repair=rebuild release bundle",
        )
        rows.append(
            {
                "lane_id": lane_id,
                "substrate": PROOF_SUBSTRATES_BY_LANE[lane_id],
                "requested_tag": requested_tag,
                "resolved_install_tag": proof_resolved_tag,
                "m80_version": proof_m80_version,
                "m80_release_tag": proof_m80_release_tag,
                "bundle_release_tag": bundle_release_tag,
                "bundle_m80_version": bundle_m80_version,
                "manifest_schema_version": manifest_schema_version,
                "guest_protocol_version": guest_protocol_version,
                "bundle_metadata": metadata_ref,
            }
        )
    return rows


def normalized_release_identity(value: object) -> list[dict[str, Any]]:
    rows = require_list(value, "release evidence bundle release_identity")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release evidence bundle release_identity row must be an object")
        require_exact_fields(row, RELEASE_IDENTITY_FIELDS, "release evidence bundle release_identity row")
        lane_id = require_lane_id(row["lane_id"], "release evidence bundle release_identity lane_id")
        expected_substrate = PROOF_SUBSTRATES_BY_LANE.get(lane_id)
        require(expected_substrate is not None, f"release evidence bundle release_identity {lane_id} lane unknown")
        require(row["substrate"] == expected_substrate, f"release evidence bundle release_identity {lane_id} substrate mismatch")
        result.append(
            {
                "lane_id": lane_id,
                "substrate": row["substrate"],
                "requested_tag": require_nonempty_string(
                    row["requested_tag"],
                    f"release evidence bundle release_identity {lane_id} requested_tag",
                ),
                "resolved_install_tag": require_nonempty_string(
                    row["resolved_install_tag"],
                    f"release evidence bundle release_identity {lane_id} resolved_install_tag",
                ),
                "m80_version": require_nonempty_string(
                    row["m80_version"],
                    f"release evidence bundle release_identity {lane_id} m80_version",
                ),
                "m80_release_tag": require_nonempty_string(
                    row["m80_release_tag"],
                    f"release evidence bundle release_identity {lane_id} m80_release_tag",
                ),
                "bundle_release_tag": require_nonempty_string(
                    row["bundle_release_tag"],
                    f"release evidence bundle release_identity {lane_id} bundle_release_tag",
                ),
                "bundle_m80_version": require_nonempty_string(
                    row["bundle_m80_version"],
                    f"release evidence bundle release_identity {lane_id} bundle_m80_version",
                ),
                "manifest_schema_version": require_positive_int(
                    row["manifest_schema_version"],
                    f"release evidence bundle release_identity {lane_id} manifest_schema_version",
                ),
                "guest_protocol_version": require_positive_int(
                    row["guest_protocol_version"],
                    f"release evidence bundle release_identity {lane_id} guest_protocol_version",
                ),
                "bundle_metadata": normalized_file_ref(
                    row["bundle_metadata"],
                    f"release evidence bundle release_identity {lane_id} bundle_metadata",
                ),
            }
        )
    lane_ids = [row["lane_id"] for row in result]
    require(len(lane_ids) == len(set(lane_ids)), "release evidence bundle release_identity duplicate lane_id")
    return sorted(result, key=lambda row: row["lane_id"])


def substrate_policy_summaries(
    *,
    dist_dir: Path,
    proof_inputs: dict[str, Path],
    lane_policy_by_id: dict[str, dict[str, Any]],
    latest_lane_ids: set[str],
    config_path: Path,
) -> list[dict[str, Any]]:
    rows = []
    for lane_id, proof_path in sorted(proof_inputs.items()):
        proof = read_json(proof_path, f"{lane_id} quickstart proof")
        lane = validate_quickstart_proof_policy(
            lane_id=lane_id,
            proof=proof,
            proof_path=proof_path,
            lane_policy_by_id=lane_policy_by_id,
            config_path=config_path,
        )
        observed_substrate = require_proof_substrate_kind(
            proof,
            f"{lane_id} quickstart proof substrate",
        )
        observed_proof_kind = require_nonempty_string(
            proof.get("proof_kind"),
            f"{lane_id} quickstart proof proof_kind",
        )
        allowed_substrates = sorted(require_string_list(lane["allowed_substrates"], f"{lane_id} allowed_substrates"))
        proof_fixture = observed_substrate in {"hostless", "local-fixture"} or bool(
            require_object(proof.get("substrate"), f"{lane_id} quickstart proof substrate").get("fixture") is True
        )
        publish_blocking = lane["publish_blocking"] is True
        may_satisfy_publish = publish_blocking and observed_substrate in allowed_substrates
        may_satisfy_latest = lane_id in latest_lane_ids and may_satisfy_publish and not proof_fixture
        rows.append(
            {
                "lane_id": lane_id,
                "proof_kind": lane["proof_kind"],
                "observed_proof_kind": observed_proof_kind,
                "observed_substrate": observed_substrate,
                "required_substrates": allowed_substrates,
                "required_substrate_class": REQUIRED_SUBSTRATE_CLASS_BY_LANE.get(lane_id, "configured"),
                "proof_fixture": proof_fixture,
                "publish_blocking": publish_blocking,
                "may_satisfy_publish": may_satisfy_publish,
                "may_satisfy_latest": may_satisfy_latest,
                "proof": file_ref(dist_dir, proof_path, f"{lane_id} proof"),
                "config": config_path.as_posix(),
            }
        )
    return rows


def normalized_substrate_policy(value: object) -> list[dict[str, Any]]:
    rows = require_list(value, "release evidence bundle substrate_policy")
    result = []
    for row in rows:
        require(isinstance(row, dict), "release evidence bundle substrate_policy row must be an object")
        require_exact_fields(row, SUBSTRATE_POLICY_FIELDS, "release evidence bundle substrate_policy row")
        lane_id = require_lane_id(row["lane_id"], "release evidence bundle substrate_policy lane_id")
        result.append(
            {
                "lane_id": lane_id,
                "proof_kind": require_nonempty_string(
                    row["proof_kind"],
                    f"release evidence bundle substrate_policy {lane_id} proof_kind",
                ),
                "observed_proof_kind": require_nonempty_string(
                    row["observed_proof_kind"],
                    f"release evidence bundle substrate_policy {lane_id} observed_proof_kind",
                ),
                "observed_substrate": require_nonempty_string(
                    row["observed_substrate"],
                    f"release evidence bundle substrate_policy {lane_id} observed_substrate",
                ),
                "required_substrates": require_string_list(
                    row["required_substrates"],
                    f"release evidence bundle substrate_policy {lane_id} required_substrates",
                ),
                "required_substrate_class": require_nonempty_string(
                    row["required_substrate_class"],
                    f"release evidence bundle substrate_policy {lane_id} required_substrate_class",
                ),
                "proof_fixture": require_bool(
                    row["proof_fixture"],
                    f"release evidence bundle substrate_policy {lane_id} proof_fixture",
                ),
                "publish_blocking": require_bool(
                    row["publish_blocking"],
                    f"release evidence bundle substrate_policy {lane_id} publish_blocking",
                ),
                "may_satisfy_publish": require_bool(
                    row["may_satisfy_publish"],
                    f"release evidence bundle substrate_policy {lane_id} may_satisfy_publish",
                ),
                "may_satisfy_latest": require_bool(
                    row["may_satisfy_latest"],
                    f"release evidence bundle substrate_policy {lane_id} may_satisfy_latest",
                ),
                "proof": normalized_file_ref(
                    row["proof"],
                    f"release evidence bundle substrate_policy {lane_id} proof",
                ),
                "config": require_nonempty_string(
                    row["config"],
                    f"release evidence bundle substrate_policy {lane_id} config",
                ),
            }
        )
    lane_ids = [row["lane_id"] for row in result]
    require(len(lane_ids) == len(set(lane_ids)), "release evidence bundle substrate_policy duplicate lane_id")
    return sorted(result, key=lambda row: row["lane_id"])


def validate_quickstart_proof_policy(
    *,
    lane_id: str,
    proof: dict[str, Any],
    proof_path: Path,
    lane_policy_by_id: dict[str, dict[str, Any]],
    config_path: Path,
) -> dict[str, Any]:
    lane = lane_policy_by_id.get(lane_id)
    require(
        lane is not None,
        f"{lane_id} substrate policy missing lane in {config_path.as_posix()}",
    )
    observed_substrate = require_proof_substrate_kind(
        proof,
        f"{lane_id} quickstart proof substrate",
    )
    allowed_substrates = set(require_string_list(lane["allowed_substrates"], f"{lane_id} allowed_substrates"))
    require(
        observed_substrate in allowed_substrates,
        f"{lane_id} substrate mismatch: required {comma_or_none(sorted(allowed_substrates))}, got {observed_substrate}; proof={proof_path.name}; config={config_path.as_posix()}",
    )
    expected_proof_kind = PROOF_PAYLOAD_KINDS_BY_LANE[lane_id]
    observed_proof_kind = require_nonempty_string(
        proof.get("proof_kind"),
        f"{lane_id} quickstart proof proof_kind",
    )
    require(
        observed_proof_kind == expected_proof_kind,
        f"{lane_id} proof_kind mismatch: expected {expected_proof_kind}, got {observed_proof_kind}; proof={proof_path.name}; config={config_path.as_posix()}",
    )
    return lane


def readiness_lanes_by_id(config: dict[str, Any]) -> dict[str, dict[str, Any]]:
    lanes = require_list(config.get("lanes"), "release readiness config lanes")
    result: dict[str, dict[str, Any]] = {}
    for lane in lanes:
        require(isinstance(lane, dict), "release readiness config lane must be an object")
        lane_id = require_lane_id(lane.get("id"), "release readiness config lane id")
        require(lane_id not in result, f"release readiness config duplicate lane id: {lane_id}")
        result[lane_id] = lane
    return result


def latest_stage_lane_ids(config: dict[str, Any]) -> set[str]:
    result: set[str] = set()
    for stage in require_list(config.get("readiness_stages"), "release readiness config readiness_stages"):
        require(isinstance(stage, dict), "release readiness config stage must be an object")
        if stage.get("id") in {"pre-latest", "post-latest-public"}:
            result.update(require_lane_ids(stage.get("required_lane_ids"), "release readiness config stage required_lane_ids"))
    return result


def require_proof_substrate_kind(proof: dict[str, Any], label: str) -> str:
    substrate = require_object(proof.get("substrate"), label)
    require_exact_fields(substrate, {"kind", "summary"}, label)
    return require_nonempty_string(substrate["kind"], f"{label} kind")


def install_root_classification(proof: dict[str, Any]) -> str:
    substrate = proof.get("proof_kind")
    if substrate == "hostless":
        return "hostless-fixture"
    install = require_object(proof.get("install"), f"{substrate} quickstart proof install")
    root = require_nonempty_string(install.get("root"), f"{substrate} quickstart proof install root")
    return "default-install-root" if root == "/opt/m80" else "override-install-root"


def require_object(value: object, label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def require_relative_artifact_path(value: object, label: str) -> str:
    require(isinstance(value, str) and value, f"{label} must be a nonempty relative path")
    path = Path(value)
    require(not path.is_absolute(), f"{label} must be relative to the proof artifact root")
    require(".." not in path.parts, f"{label} must not escape the proof artifact root")
    return path.as_posix()


def reject_absolute_host_path_leak(value: object, label: str) -> None:
    if isinstance(value, dict):
        for key, item in value.items():
            reject_absolute_host_path_leak(item, f"{label}.{key}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            reject_absolute_host_path_leak(item, f"{label}[{index}]")
    elif isinstance(value, str):
        require(not looks_like_absolute_host_path(value), f"{label} leaks absolute host path")


def looks_like_absolute_host_path(value: str) -> bool:
    return re.search(r"(^|[\s=:\"'(\[])/(home|root|tmp|var|opt|tank)/", value) is not None


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


def verify_file_ref(
    value: object,
    dist_dir: Path,
    path: Path,
    *,
    field_path: str,
    label: str,
    repair_command: str,
) -> dict[str, Any]:
    observed = normalized_file_ref(value, f"release evidence bundle {label}")
    require(
        path.is_file(),
        (
            f"release evidence bundle {field_path} referenced file missing: expected_name={path.name} "
            f"expected_sha256=<missing> actual_sha256={observed['sha256']} "
            f"expected_size_bytes=<missing> actual_size_bytes={observed['size_bytes']} "
            f"repair={repair_command}"
        ),
    )
    try:
        path.relative_to(dist_dir)
    except ValueError as exc:
        raise SystemExit(
            (
                f"release evidence bundle {field_path} path outside artifact root: "
                + diagnostic_mismatch(
                    f"{field_path}.path",
                    path.name,
                    "artifact-root-relative file",
                    path.as_posix(),
                    repair_command,
                )
            )
        ) from exc
    expected = file_ref(dist_dir, path, label)
    for key, noun in (("name", "name"), ("sha256", "digest"), ("size_bytes", "size")):
        require(
            observed[key] == expected[key],
            (
                f"release evidence bundle {field_path} {noun} mismatch: "
                f"expected_name={expected['name']} actual_name={observed['name']} "
                f"expected_sha256={expected['sha256']} actual_sha256={observed['sha256']} "
                f"expected_size_bytes={expected['size_bytes']} actual_size_bytes={observed['size_bytes']} "
                f"repair={repair_command}"
            ),
        )
    return expected


def diagnostic_mismatch(
    field_path: str,
    source: str,
    expected: object,
    observed: object,
    repair_command: str,
) -> str:
    return (
        f"release evidence bundle {field_path} mismatch: "
        f"field={field_path} source={source} "
        f"expected={diagnostic_value(expected)} observed={diagnostic_value(observed)} "
        f"repair={repair_command}"
    )


def diagnostic_value(value: object) -> str:
    if value is None:
        text = "null"
    elif isinstance(value, (dict, list)):
        text = json.dumps(value, sort_keys=True, separators=(",", ":"))
    else:
        text = str(value)
    if SECRET_VALUE_RE.search(text) or ABSOLUTE_HOST_PATH_RE.search(text):
        return "<redacted>"
    if len(text) > DIAGNOSTIC_VALUE_LIMIT:
        return f"{text[:DIAGNOSTIC_VALUE_LIMIT]}...<truncated {len(text)} chars>"
    return text


def verify_bundle_path(bundle_path: Path, dist_dir: Path) -> None:
    require(bundle_path.is_file(), f"release evidence bundle missing: {bundle_path}")
    try:
        rel = bundle_path.relative_to(dist_dir)
    except ValueError as exc:
        raise SystemExit(f"release evidence bundle path outside artifact root: {bundle_path}") from exc
    require(len(rel.parts) == 1, f"release evidence bundle path must be a top-level dist file: {bundle_path}")
    name = require_dist_name(rel.name, "release evidence bundle name")
    require(
        name == EVIDENCE_BUNDLE_NAME,
        f"release evidence bundle path mismatch: expected {EVIDENCE_BUNDLE_NAME}, got {name}",
    )


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


def require_bool(value: object, label: str) -> bool:
    require(isinstance(value, bool), f"{label} must be a boolean")
    return value


def require_list(value: object, label: str) -> list[object]:
    require(isinstance(value, list), f"{label} must be a list")
    return value


def require_non_negative_int(value: object, label: str) -> None:
    require(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")


def require_positive_int(value: object, label: str) -> int:
    require(isinstance(value, int) and value > 0, f"{label} must be a positive integer")
    return value


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


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


if __name__ == "__main__":
    raise SystemExit(main())
