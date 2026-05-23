#!/usr/bin/env python3
"""Merge release-readiness lane receipts into one publish decision."""

from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
KIND = "m80_release_readiness_decision"
DEFAULT_CONFIG = Path("docs/behaviors/release/release-readiness-lanes.json")
SCRIPT_DIR = Path(__file__).resolve().parent


class VerificationError(ValueError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--workflow-run-id")
    parser.add_argument("--receipt", action="append", default=[], help="lane_id=path")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        config = read_json(args.config, "release readiness config")
        errors = readiness_config_module().validate_readiness_config(config, source=args.config)
        if errors:
            raise VerificationError("; ".join(errors))
        receipt_paths = parse_receipt_args(args.receipt)
        if args.write:
            decision = build_decision(
                config,
                receipt_paths,
                release_tag=args.release_tag,
                commit_sha=args.commit_sha,
                workflow_run_id=args.workflow_run_id,
            )
            write_json(args.out, decision)
        decision = read_json(args.out, "release readiness decision")
        verify_decision(
            decision,
            config,
            receipt_paths,
            release_tag=args.release_tag,
            commit_sha=args.commit_sha,
            workflow_run_id=args.workflow_run_id,
        )
    except VerificationError as exc:
        raise SystemExit(str(exc)) from exc
    print(f"release readiness passed: {args.out}")
    return 0


def build_decision(
    config: dict[str, Any],
    receipt_paths: dict[str, Path],
    *,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str | None,
) -> dict[str, Any]:
    lanes = lanes_by_id(config)
    required_ids = required_lane_ids(config)
    missing = sorted(required_ids - set(receipt_paths))
    check(not missing, f"missing required readiness lane(s): {', '.join(missing)}")

    summaries = []
    blocking = []
    for lane_id, path in sorted(receipt_paths.items()):
        lane = lanes.get(lane_id)
        check(lane is not None, f"unknown lane id: {lane_id}")
        receipt = read_json(path, f"readiness receipt {lane_id}")
        summary = summarize_lane(
            lane,
            receipt,
            path=path,
            release_tag=release_tag,
            commit_sha=commit_sha,
            workflow_run_id=workflow_run_id,
        )
        summaries.append(summary)
        if summary["publish_effect"] == "blocks":
            blocking.append(
                {
                    "lane_id": lane_id,
                    "status": summary["status"],
                    "remediation": summary["remediation"],
                }
            )

    check(not blocking, "blocking readiness lane(s): " + ", ".join(row["lane_id"] for row in blocking))
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "status": "passed",
        "release_tag": release_tag,
        "commit_sha": commit_sha,
        "workflow_run_id": workflow_run_id,
        "required_lane_ids": sorted(required_ids),
        "lane_status": summaries,
        "blocking_remediations": blocking,
    }


def verify_decision(
    decision: dict[str, Any],
    config: dict[str, Any],
    receipt_paths: dict[str, Path],
    *,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str | None,
) -> None:
    expected = build_decision(
        config,
        receipt_paths,
        release_tag=release_tag,
        commit_sha=commit_sha,
        workflow_run_id=workflow_run_id,
    )
    check(decision == expected, "release readiness decision drift")


def summarize_lane(
    lane: dict[str, Any],
    receipt: dict[str, Any],
    *,
    path: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str | None,
) -> dict[str, Any]:
    lane_id = lane["id"]
    check(receipt.get("lane_id") == lane_id, f"{lane_id}: receipt lane_id mismatch")
    status = receipt.get("status")
    status_row = status_by_id(lane, status)
    check(receipt.get("release_tag") == release_tag, f"{lane_id}: stale release tag")
    check(receipt.get("commit_sha") == commit_sha, f"{lane_id}: stale commit sha")
    if workflow_run_id is not None and receipt.get("workflow_run_id") is not None:
        check(receipt.get("workflow_run_id") == workflow_run_id, f"{lane_id}: stale workflow run id")

    substrate = require_object(receipt.get("substrate"), f"{lane_id}: substrate")
    substrate_kind = substrate.get("kind")
    fixture = substrate.get("fixture")
    check(substrate_kind in lane["allowed_substrates"], f"{lane_id}: wrong-substrate: {substrate_kind}")
    check(isinstance(fixture, bool), f"{lane_id}: substrate.fixture must be boolean")
    if lane["publish_blocking"] and lane["allowed_substrates"] != ["hostless"]:
        check(fixture is False, f"{lane_id}: fixture receipt cannot satisfy required substrate")
    if lane_id == "public-access-latest":
        verify_public_access_auth(receipt)

    digest_field = lane["digest_field"]
    digest = receipt.get(digest_field)
    check(is_digest(digest), f"{lane_id}: missing digest field: {digest_field}")
    artifact_paths = artifact_paths_for_receipt(receipt)
    remediation = remediation_for_receipt(lane, receipt)
    publish_effect = status_row["publish_effect"]
    if lane["publish_blocking"] and status != "passed":
        publish_effect = "blocks"
    return {
        "lane_id": lane_id,
        "lane_kind": lane["lane_kind"],
        "proof_kind": lane["proof_kind"],
        "status": status,
        "publish_effect": publish_effect,
        "severity": lane["severity"],
        "publish_blocking": lane["publish_blocking"],
        "receipt_path": path.as_posix(),
        "artifact_paths": artifact_paths,
        "digest_field": digest_field,
        "digest": digest,
        "substrate": {
            "kind": substrate_kind,
            "fixture": fixture,
        },
        "remediation": remediation,
    }


def status_by_id(lane: dict[str, Any], status_id: object) -> dict[str, Any]:
    config_statuses = readiness_config_module().REQUIRED_STATUS_IDS
    check(isinstance(status_id, str) and status_id in config_statuses, f"{lane['id']}: unknown lane state: {status_id}")
    if status_id == "passed":
        return {"publish_effect": "satisfies"}
    if status_id == "warning":
        return {"publish_effect": "warn"}
    return {"publish_effect": "blocks"}


def artifact_paths_for_receipt(receipt: dict[str, Any]) -> list[str]:
    artifact = receipt.get("artifact")
    if isinstance(artifact, dict) and isinstance(artifact.get("path"), str):
        return [artifact["path"]]
    asset_manifest = receipt.get("asset_manifest")
    if isinstance(asset_manifest, dict) and isinstance(asset_manifest.get("name"), str):
        return [asset_manifest["name"]]
    sources = receipt.get("snippet_sources")
    if isinstance(sources, list):
        return [row["path"] for row in sources if isinstance(row, dict) and isinstance(row.get("path"), str)]
    return []


def remediation_for_receipt(lane: dict[str, Any], receipt: dict[str, Any]) -> dict[str, Any]:
    remediation = receipt.get("remediation")
    if isinstance(remediation, dict):
        command = remediation.get("command")
        bead_id = remediation.get("bead_id")
    else:
        command = lane["remediation"]["command"]
        bead_id = None
    return {
        "command": command if isinstance(command, str) else None,
        "bead_id": bead_id if isinstance(bead_id, str) else None,
    }


def verify_public_access_auth(receipt: dict[str, Any]) -> None:
    auth = require_object(receipt.get("auth"), "public-access auth")
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "gh_auth_present", "authorization_header_used"]:
        check(auth.get(key) is False, f"public-access proof used auth: {key}")


def parse_receipt_args(values: list[str]) -> dict[str, Path]:
    receipts: dict[str, Path] = {}
    for value in values:
        lane_id, sep, path = value.partition("=")
        check(bool(sep and lane_id and path), f"receipt must be lane_id=path: {value}")
        check(lane_id not in receipts, f"duplicate receipt lane id: {lane_id}")
        receipts[lane_id] = Path(path)
    return receipts


def required_lane_ids(config: dict[str, Any]) -> set[str]:
    return {
        lane["id"]
        for lane in config["lanes"]
        if lane.get("severity") == "required" and lane.get("publish_blocking") is True
    }


def lanes_by_id(config: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {lane["id"]: lane for lane in config["lanes"]}


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


def require_object(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise VerificationError(f"{label} must be an object")
    return value


def is_digest(value: object) -> bool:
    if not isinstance(value, str) or not value.startswith("sha256:"):
        return False
    digest = value[len("sha256:") :]
    return len(digest) == 64 and all(char in "0123456789abcdef" for char in digest)


def check(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


if __name__ == "__main__":
    raise SystemExit(main())
