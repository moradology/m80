#!/usr/bin/env python3
"""Build and verify normalized local release-readiness lane receipts."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
from typing import Any


SCHEMA_VERSION = 1
KIND = "m80_release_readiness_lane_receipt"
DEFAULT_CONFIG = Path("docs/behaviors/release/release-readiness-lanes.json")
LOCAL_LANE_IDS = {"workflow-policy", "release-bundle-integrity", "hostless-quickstart"}
SCRIPT_DIR = Path(__file__).resolve().parent
SHA256_PREFIX = "sha256:"


class VerificationError(ValueError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--lane-id", required=True)
    parser.add_argument("--status", required=True)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--workflow-run-id")
    parser.add_argument("--substrate-kind", required=True)
    parser.add_argument("--artifact-root", type=Path, required=True)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expected-release-tag")
    parser.add_argument("--expected-commit-sha")
    parser.add_argument("--remediation-command")
    parser.add_argument("--remediation-bead-id")
    parser.add_argument("--verification-time")
    parser.add_argument("--fixture", action="store_true")
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        config = read_json(args.config, "release readiness config")
        config_errors = readiness_config_module().validate_readiness_config(config, source=args.config)
        if config_errors:
            raise VerificationError("; ".join(config_errors))
        if args.write:
            receipt = build_receipt(args, config)
            write_json(args.out, receipt)
        receipt = read_json(args.out, "release readiness lane receipt")
        verify_receipt(
            receipt,
            config,
            expected_lane_id=args.lane_id,
            expected_release_tag=args.expected_release_tag or args.release_tag,
            expected_commit_sha=args.expected_commit_sha or args.commit_sha,
            expected_workflow_run_id=args.workflow_run_id,
        )
    except VerificationError as exc:
        raise SystemExit(str(exc)) from exc
    print(f"release readiness lane receipt ok: {args.out}")
    return 0


def build_receipt(args: argparse.Namespace, config: dict[str, Any]) -> dict[str, Any]:
    lane = find_lane(config, args.lane_id)
    check(args.lane_id in LOCAL_LANE_IDS, f"unsupported local readiness lane: {args.lane_id}")
    check(args.status in status_ids(config), f"unknown status: {args.status}")
    check(args.substrate_kind in lane["allowed_substrates"], f"wrong-substrate: {args.substrate_kind}")
    check(not stronger_fixture_claim(args.fixture, args.substrate_kind), "wrong-substrate: fixture cannot claim substrate")

    artifact = artifact_row(args.artifact_root, args.artifact)
    digest = artifact["sha256"]
    remediation_command = args.remediation_command or lane["remediation"]["command"]
    validate_remediation_command(remediation_command, args.lane_id)
    remediation = {
        "command": remediation_command,
        "bead_id": args.remediation_bead_id,
    }
    if status_requires_remediation(config, args.status):
        check(remediation["command"] or remediation["bead_id"], "remediation required for blocking status")

    receipt = {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "lane_id": lane["id"],
        "lane_kind": lane["lane_kind"],
        "proof_kind": lane["proof_kind"],
        "status": args.status,
        "release_tag": args.release_tag,
        "commit_sha": args.commit_sha,
        "workflow_run_id": args.workflow_run_id,
        "verification_time": args.verification_time or utc_now(),
        "substrate": {
            "kind": args.substrate_kind,
            "fixture": bool(args.fixture),
        },
        "artifact": artifact,
        lane["digest_field"]: digest,
        "remediation": remediation,
    }
    verify_receipt(
        receipt,
        config,
        expected_lane_id=args.lane_id,
        expected_release_tag=args.expected_release_tag or args.release_tag,
        expected_commit_sha=args.expected_commit_sha or args.commit_sha,
        expected_workflow_run_id=args.workflow_run_id,
    )
    return receipt


def verify_receipt(
    receipt: dict[str, Any],
    config: dict[str, Any],
    *,
    expected_lane_id: str,
    expected_release_tag: str,
    expected_commit_sha: str,
    expected_workflow_run_id: str | None,
) -> None:
    lane = find_lane(config, expected_lane_id)
    expected_fields = {
        "schema_version",
        "kind",
        "lane_id",
        "lane_kind",
        "proof_kind",
        "status",
        "release_tag",
        "commit_sha",
        "workflow_run_id",
        "verification_time",
        "substrate",
        "artifact",
        lane["digest_field"],
        "remediation",
    }
    require_exact_fields(receipt, expected_fields, "release readiness lane receipt")
    check(receipt["schema_version"] == SCHEMA_VERSION, "unsupported receipt schema_version")
    check(receipt["kind"] == KIND, "unknown receipt kind")
    check(receipt["lane_id"] == expected_lane_id, "unknown lane id or lane mismatch")
    check(receipt["lane_kind"] == lane["lane_kind"], "lane_kind mismatch")
    check(receipt["proof_kind"] == lane["proof_kind"], "proof_kind mismatch")
    check(receipt["status"] in status_ids(config), f"unknown status: {receipt['status']}")
    check(receipt["release_tag"] == expected_release_tag, "stale release tag")
    check(receipt["commit_sha"] == expected_commit_sha, "stale commit sha")
    if expected_workflow_run_id is not None:
        check(receipt["workflow_run_id"] == expected_workflow_run_id, "stale workflow run id")

    substrate = require_object(receipt["substrate"], "substrate")
    require_exact_fields(substrate, {"kind", "fixture"}, "substrate")
    check(substrate["kind"] in lane["allowed_substrates"], f"wrong-substrate: {substrate['kind']}")
    check(isinstance(substrate["fixture"], bool), "substrate.fixture must be boolean")
    check(not stronger_fixture_claim(substrate["fixture"], substrate["kind"]), "wrong-substrate: fixture cannot claim substrate")

    artifact = require_object(receipt["artifact"], "artifact")
    require_exact_fields(artifact, {"path", "sha256", "size_bytes"}, "artifact")
    check(isinstance(artifact["path"], str) and artifact["path"], "artifact.path must be nonempty")
    check(is_digest(artifact["sha256"]), "artifact.sha256 must be sha256:<hex>")
    check(isinstance(artifact["size_bytes"], int) and artifact["size_bytes"] >= 0, "artifact.size_bytes must be nonnegative")
    check(receipt[lane["digest_field"]] == artifact["sha256"], "missing digest field or digest mismatch")

    remediation = require_object(receipt["remediation"], "remediation")
    require_exact_fields(remediation, {"command", "bead_id"}, "remediation")
    if remediation["command"] is not None:
        check(isinstance(remediation["command"], str) and remediation["command"], "remediation.command must be nonempty")
        validate_remediation_command(remediation["command"], expected_lane_id)
    if remediation["bead_id"] is not None:
        check(isinstance(remediation["bead_id"], str) and remediation["bead_id"].startswith("m80-"), "invalid remediation bead id")
    if status_requires_remediation(config, receipt["status"]):
        check(remediation["command"] or remediation["bead_id"], "remediation required for blocking status")


def artifact_row(root: Path, artifact: Path) -> dict[str, Any]:
    root_resolved = root.resolve()
    path_resolved = artifact.resolve()
    try:
        rel = path_resolved.relative_to(root_resolved)
    except ValueError as exc:
        raise VerificationError("artifact path escapes artifact root") from exc
    check(path_resolved.is_file(), f"artifact file missing: {artifact}")
    data = path_resolved.read_bytes()
    return {
        "path": rel.as_posix(),
        "sha256": f"{SHA256_PREFIX}{hashlib.sha256(data).hexdigest()}",
        "size_bytes": len(data),
    }


def find_lane(config: dict[str, Any], lane_id: str) -> dict[str, Any]:
    for lane in config.get("lanes", []):
        if isinstance(lane, dict) and lane.get("id") == lane_id:
            return lane
    raise VerificationError(f"unknown lane id: {lane_id}")


def status_ids(config: dict[str, Any]) -> set[str]:
    return {status["id"] for status in config.get("status_taxonomy", []) if isinstance(status, dict)}


def status_requires_remediation(config: dict[str, Any], status_id: str) -> bool:
    for status in config.get("status_taxonomy", []):
        if isinstance(status, dict) and status.get("id") == status_id:
            return bool(status.get("requires_remediation"))
    raise VerificationError(f"unknown status: {status_id}")


def stronger_fixture_claim(fixture: bool, substrate_kind: str) -> bool:
    return fixture and substrate_kind in {"github-actions", "real-kvm", "public-github"}


def validate_remediation_command(command: str, lane_id: str) -> None:
    errors = readiness_config_module().validate_remediation_command(command, f"lane {lane_id}")
    if errors:
        raise VerificationError("; ".join(errors))


def readiness_config_module():
    module_path = SCRIPT_DIR / "verify-release-readiness-config.py"
    spec = importlib.util.spec_from_file_location("verify_release_readiness_config", module_path)
    if spec is None or spec.loader is None:
        raise VerificationError(f"cannot import {module_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_json(path: Path, label: str) -> dict[str, Any]:
    try:
        with path.open() as f:
            value = json.load(f)
    except FileNotFoundError as exc:
        raise VerificationError(f"{label} missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise VerificationError(f"{label} invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise VerificationError(f"{label} must be a JSON object")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def require_exact_fields(obj: dict[str, Any], fields: set[str], label: str) -> None:
    actual = set(obj)
    missing = sorted(fields - actual)
    extra = sorted(actual - fields)
    if missing:
        raise VerificationError(f"{label}: missing field(s): {', '.join(missing)}")
    if extra:
        raise VerificationError(f"{label}: unknown field(s): {', '.join(extra)}")


def require_object(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise VerificationError(f"{label} must be an object")
    return value


def is_digest(value: Any) -> bool:
    if not isinstance(value, str) or not value.startswith(SHA256_PREFIX):
        return False
    digest = value[len(SHA256_PREFIX) :]
    return len(digest) == 64 and all(char in "0123456789abcdef" for char in digest)


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def check(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


if __name__ == "__main__":
    raise SystemExit(main())
