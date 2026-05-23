#!/usr/bin/env python3
"""Validate the release readiness lane contract."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shlex
import sys
from typing import Any


DEFAULT_CONFIG = Path("docs/behaviors/release/release-readiness-lanes.json")
SCHEMA_VERSION = 1
TOP_LEVEL_FIELDS = {"schema_version", "status_taxonomy", "readiness_stages", "lanes"}
STATUS_FIELDS = {"id", "publish_effect", "requires_remediation", "description"}
STAGE_FIELDS = {"id", "required_lane_ids", "policy_text"}
LANE_FIELDS = {
    "id",
    "title",
    "lane_kind",
    "proof_kind",
    "allowed_substrates",
    "severity",
    "publish_blocking",
    "expected_tag_field",
    "expected_commit_field",
    "digest_field",
    "remediation",
    "policy_text",
}
REMEDIATION_FIELDS = {"field", "command"}
LANE_KINDS = {
    "workflow-policy",
    "docs-command",
    "release-integrity",
    "hostless-quickstart",
    "real-kvm-smoke",
    "public-access",
    "freshness",
}
PROOF_KINDS = {
    "workflow-policy-report",
    "docs-command-receipt",
    "release-integrity-predicate",
    "quickstart-proof",
    "public-access-proof",
    "freshness-proof",
}
SUBSTRATE_KINDS = {"github-actions", "hostless", "real-kvm", "public-github", "local-fixture"}
SEVERITIES = {"required", "optional", "warning"}
PUBLISH_EFFECTS = {"satisfies", "warn", "blocks"}
REQUIRED_STATUS_IDS = {"passed", "warning", "failed", "stale", "missing", "unauthenticated", "wrong-substrate"}
REQUIRED_LANE_IDS = {
    "workflow-policy",
    "docs-command",
    "release-bundle-integrity",
    "hostless-quickstart",
    "real-kvm-quickstart",
    "public-access-latest",
}
REQUIRED_STAGE_LANES = {
    "pre-upload": {
        "workflow-policy",
        "docs-command",
        "release-bundle-integrity",
        "hostless-quickstart",
    },
    "pre-latest": {
        "workflow-policy",
        "docs-command",
        "release-bundle-integrity",
        "hostless-quickstart",
    },
    "post-latest-public": REQUIRED_LANE_IDS,
}
EXPECTED_PROOF_BY_KIND = {
    "workflow-policy": {"workflow-policy-report"},
    "docs-command": {"docs-command-receipt"},
    "release-integrity": {"release-integrity-predicate"},
    "hostless-quickstart": {"quickstart-proof"},
    "real-kvm-smoke": {"quickstart-proof"},
    "public-access": {"public-access-proof"},
    "freshness": {"freshness-proof"},
}
EXPECTED_SUBSTRATES_BY_KIND = {
    "workflow-policy": {"github-actions"},
    "docs-command": {"github-actions"},
    "release-integrity": {"github-actions"},
    "hostless-quickstart": {"hostless"},
    "real-kvm-smoke": {"real-kvm"},
    "public-access": {"public-github"},
    "freshness": {"public-github"},
}
ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
FIELD_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$")
FORBIDDEN_COMMAND_FRAGMENTS = ["\n", "\r", ";", "&&", "||", "|", "`", "$(", ">"]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help="release readiness lane JSON config",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    errors = validate_readiness_config(read_json(args.config), source=args.config)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"release readiness config ok: {args.config}")
    return 0


def read_json(path: Path) -> dict[str, Any]:
    try:
        with path.open() as f:
            value = json.load(f)
    except FileNotFoundError as exc:
        raise SystemExit(f"release readiness config missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: release readiness config must be a JSON object")
    return value


def validate_readiness_config(config: dict[str, Any], *, source: Path | str = "<config>") -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(config, TOP_LEVEL_FIELDS, str(source)))
    if config.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"{source}: schema_version must be {SCHEMA_VERSION}")

    statuses = config.get("status_taxonomy")
    stages = config.get("readiness_stages")
    lanes = config.get("lanes")
    if isinstance(statuses, list):
        errors.extend(validate_status_taxonomy(statuses, source=source))
    else:
        errors.append(f"{source}: status_taxonomy must be a nonempty list")
    if not isinstance(lanes, list) or not lanes:
        errors.append(f"{source}: lanes must be a nonempty list")
        return errors

    seen_ids: set[str] = set()
    present_ids: set[str] = set()
    for index, lane in enumerate(lanes):
        label = f"{source}:lanes[{index}]"
        if not isinstance(lane, dict):
            errors.append(f"{label}: lane must be an object")
            continue
        lane_id = lane.get("id")
        if isinstance(lane_id, str) and lane_id:
            label = f"{source}:lane {lane_id}"
            if lane_id in seen_ids:
                errors.append(f"{label}: duplicate lane id")
            seen_ids.add(lane_id)
            present_ids.add(lane_id)
        errors.extend(validate_lane(lane, label))

    for required_id in sorted(REQUIRED_LANE_IDS - present_ids):
        errors.append(f"{source}: missing required readiness lane: {required_id}")
    if isinstance(stages, list):
        errors.extend(validate_readiness_stages(stages, lane_ids=present_ids, lanes=lanes, source=source))
    else:
        errors.append(f"{source}: readiness_stages must be a nonempty list")
    return errors


def validate_readiness_stages(
    stages: list[object],
    *,
    lane_ids: set[str],
    lanes: list[object],
    source: Path | str,
) -> list[str]:
    errors: list[str] = []
    if not stages:
        return [f"{source}: readiness_stages must be a nonempty list"]
    lanes_by_id = {lane["id"]: lane for lane in lanes if isinstance(lane, dict) and isinstance(lane.get("id"), str)}
    seen_ids: set[str] = set()
    present_ids: set[str] = set()
    for index, stage in enumerate(stages):
        label = f"{source}:readiness_stages[{index}]"
        if not isinstance(stage, dict):
            errors.append(f"{label}: stage must be an object")
            continue
        stage_id = stage.get("id")
        if isinstance(stage_id, str) and stage_id:
            label = f"{source}:stage {stage_id}"
            if stage_id in seen_ids:
                errors.append(f"{label}: duplicate stage id")
            seen_ids.add(stage_id)
            present_ids.add(stage_id)
        errors.extend(validate_stage(stage, label, lane_ids=lane_ids, lanes_by_id=lanes_by_id))
    for required_id in sorted(set(REQUIRED_STAGE_LANES) - present_ids):
        errors.append(f"{source}: missing required readiness stage: {required_id}")
    return errors


def validate_stage(
    stage: dict[str, Any],
    label: str,
    *,
    lane_ids: set[str],
    lanes_by_id: dict[str, dict[str, Any]],
) -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(stage, STAGE_FIELDS, label))
    stage_id = require_nonempty_string(stage, "id", label, errors)
    if stage_id and ID_RE.fullmatch(stage_id) is None:
        errors.append(f"{label}: id must be kebab-case alphanumeric")
    required_lane_ids = require_string_list(stage, "required_lane_ids", label, errors)
    if len(required_lane_ids) != len(set(required_lane_ids)):
        errors.append(f"{label}: duplicate required lane id")
    for lane_id in sorted(set(required_lane_ids) - lane_ids):
        errors.append(f"{label}: unknown required lane id: {lane_id}")
    for lane_id in required_lane_ids:
        lane = lanes_by_id.get(lane_id)
        if lane and (lane.get("severity") != "required" or lane.get("publish_blocking") is not True):
            errors.append(f"{label}: stage lane must be required and publish_blocking: {lane_id}")
    if stage_id in REQUIRED_STAGE_LANES and set(required_lane_ids) != REQUIRED_STAGE_LANES[stage_id]:
        errors.append(
            f"{label}: required_lane_ids must be {sorted(REQUIRED_STAGE_LANES[stage_id])}, "
            f"got {sorted(required_lane_ids)}"
        )
    require_nonempty_string(stage, "policy_text", label, errors)
    return errors


def validate_status_taxonomy(statuses: list[object], *, source: Path | str) -> list[str]:
    errors: list[str] = []
    if not statuses:
        return [f"{source}: status_taxonomy must be a nonempty list"]
    seen_ids: set[str] = set()
    present_ids: set[str] = set()
    for index, status in enumerate(statuses):
        label = f"{source}:status_taxonomy[{index}]"
        if not isinstance(status, dict):
            errors.append(f"{label}: status entry must be an object")
            continue
        status_id = status.get("id")
        if isinstance(status_id, str) and status_id:
            label = f"{source}:status {status_id}"
            if status_id in seen_ids:
                errors.append(f"{label}: duplicate status id")
            seen_ids.add(status_id)
            present_ids.add(status_id)
        errors.extend(validate_status(status, label))
    for required_id in sorted(REQUIRED_STATUS_IDS - present_ids):
        errors.append(f"{source}: missing required status: {required_id}")
    return errors


def validate_status(status: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(status, STATUS_FIELDS, label))
    status_id = require_nonempty_string(status, "id", label, errors)
    if status_id and ID_RE.fullmatch(status_id) is None:
        errors.append(f"{label}: id must be kebab-case alphanumeric")
    require_member(status, "publish_effect", PUBLISH_EFFECTS, label, errors)
    if not isinstance(status.get("requires_remediation"), bool):
        errors.append(f"{label}: requires_remediation must be a boolean")
    require_nonempty_string(status, "description", label, errors)
    return errors


def validate_lane(lane: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(lane, LANE_FIELDS, label))

    lane_id = require_nonempty_string(lane, "id", label, errors)
    if lane_id and ID_RE.fullmatch(lane_id) is None:
        errors.append(f"{label}: id must be kebab-case alphanumeric")
    require_nonempty_string(lane, "title", label, errors)

    lane_kind = require_member(lane, "lane_kind", LANE_KINDS, label, errors)
    proof_kind = require_member(lane, "proof_kind", PROOF_KINDS, label, errors)
    if lane_kind and proof_kind and proof_kind not in EXPECTED_PROOF_BY_KIND[lane_kind]:
        errors.append(f"{label}: proof_kind {proof_kind} is not valid for lane_kind {lane_kind}")

    allowed_substrates = require_string_list(lane, "allowed_substrates", label, errors)
    unknown_substrates = sorted(set(allowed_substrates) - SUBSTRATE_KINDS)
    for substrate in unknown_substrates:
        errors.append(f"{label}: unknown substrate kind: {substrate}")
    if lane_kind and allowed_substrates:
        expected = EXPECTED_SUBSTRATES_BY_KIND[lane_kind]
        actual = set(allowed_substrates)
        if actual != expected:
            errors.append(
                f"{label}: lane_kind {lane_kind} requires substrate(s) "
                f"{sorted(expected)}, got {sorted(actual)}"
            )
    if lane.get("publish_blocking") is True and "local-fixture" in allowed_substrates:
        errors.append(f"{label}: publish-blocking lanes cannot be satisfied by local-fixture")

    severity = require_member(lane, "severity", SEVERITIES, label, errors)
    publish_blocking = lane.get("publish_blocking")
    if not isinstance(publish_blocking, bool):
        errors.append(f"{label}: publish_blocking must be a boolean")
    elif severity == "required" and not publish_blocking:
        errors.append(f"{label}: required lanes must be publish_blocking")
    elif severity == "optional" and publish_blocking:
        errors.append(f"{label}: optional lanes cannot be publish_blocking")

    require_field_path(lane, "expected_tag_field", label, errors, nullable=False)
    require_field_path(lane, "expected_commit_field", label, errors, nullable=True)
    digest_field = require_field_path(lane, "digest_field", label, errors, nullable=False)
    if publish_blocking is True and not digest_field:
        errors.append(f"{label}: publish-blocking lanes must name digest_field")

    policy_text = lane.get("policy_text")
    if not isinstance(policy_text, str):
        errors.append(f"{label}: policy_text must be a string")
    elif severity == "warning" and publish_blocking is True and not policy_text.strip():
        errors.append(f"{label}: warning publish-blocking lane requires policy_text")

    errors.extend(validate_remediation(lane.get("remediation"), label))
    return errors


def validate_remediation(value: object, label: str) -> list[str]:
    errors: list[str] = []
    if not isinstance(value, dict):
        return [f"{label}: remediation must be an object"]
    errors.extend(require_exact_fields(value, REMEDIATION_FIELDS, f"{label}:remediation"))
    require_field_path(value, "field", f"{label}:remediation", errors, nullable=False)
    command = require_nonempty_string(value, "command", f"{label}:remediation", errors)
    if command:
        errors.extend(validate_remediation_command(command, label))
    return errors


def validate_remediation_command(command: str, label: str) -> list[str]:
    errors: list[str] = []
    for fragment in FORBIDDEN_COMMAND_FRAGMENTS:
        if fragment in command:
            errors.append(f"{label}: malformed remediation command contains {fragment!r}")
    try:
        argv = shlex.split(command)
    except ValueError as exc:
        return [f"{label}: malformed remediation command: {exc}"]
    if not argv:
        return [f"{label}: remediation command must not be empty"]
    if command_prefix_allowed(argv):
        return errors
    errors.append(f"{label}: malformed remediation command starts with unsupported tool: {argv[0]}")
    return errors


def command_prefix_allowed(argv: list[str]) -> bool:
    if argv[:2] in (["br", "show"], ["br", "update"], ["gh", "run"]):
        return True
    if argv[0] == "python3" and len(argv) >= 2 and argv[1].startswith("scripts/"):
        return True
    if argv[0].startswith("scripts/"):
        return True
    if argv[0] == "cargo" and len(argv) >= 2 and argv[1] in {"build", "test", "clippy"}:
        return True
    return False


def require_exact_fields(obj: dict[str, Any], fields: set[str], label: str) -> list[str]:
    actual = set(obj)
    errors: list[str] = []
    missing = sorted(fields - actual)
    extra = sorted(actual - fields)
    if missing:
        errors.append(f"{label}: missing field(s): {', '.join(missing)}")
    if extra:
        errors.append(f"{label}: unknown field(s): {', '.join(extra)}")
    return errors


def require_nonempty_string(
    obj: dict[str, Any],
    field: str,
    label: str,
    errors: list[str],
) -> str | None:
    value = obj.get(field)
    if not isinstance(value, str) or not value:
        errors.append(f"{label}: {field} must be a nonempty string")
        return None
    return value


def require_member(
    obj: dict[str, Any],
    field: str,
    allowed: set[str],
    label: str,
    errors: list[str],
) -> str | None:
    value = obj.get(field)
    if not isinstance(value, str) or value not in allowed:
        errors.append(f"{label}: unknown {field}: {value}")
        return None
    return value


def require_string_list(
    obj: dict[str, Any],
    field: str,
    label: str,
    errors: list[str],
) -> list[str]:
    value = obj.get(field)
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item for item in value):
        errors.append(f"{label}: {field} must be a nonempty string list")
        return []
    return value


def require_field_path(
    obj: dict[str, Any],
    field: str,
    label: str,
    errors: list[str],
    *,
    nullable: bool,
) -> str | None:
    value = obj.get(field)
    if value is None and nullable:
        return None
    if not isinstance(value, str) or not value:
        errors.append(f"{label}: {field} must be a nonempty field path")
        return None
    if FIELD_RE.fullmatch(value) is None:
        errors.append(f"{label}: {field} must be a dotted field path")
        return None
    return value


if __name__ == "__main__":
    raise SystemExit(main())
