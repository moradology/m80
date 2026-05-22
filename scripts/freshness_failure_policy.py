#!/usr/bin/env python3
"""Validate the latest freshness failure policy taxonomy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

from release_freshness import FRESHNESS_FAILURE_CLASSES


DEFAULT_CONFIG = Path("docs/behaviors/release/freshness-failure-policy.json")
SCHEMA_VERSION = 1
TOP_LEVEL_FIELDS = {"schema_version", "failure_classes"}
CLASS_FIELDS = {
    "id",
    "description",
    "disposition",
    "owner",
    "blocks_next_release",
    "opens_bead",
    "pages_maintainer",
    "requires_manual_confirmation",
    "retry_count",
    "repair_command",
    "safe_summary_fields",
}
DISPOSITIONS = {
    "retry-only",
    "open-update-bead",
    "block-next-release-latest",
    "page-maintainer",
    "manual-operator-confirmation",
}
SAFE_SUMMARY_FIELDS = {
    "artifact_path",
    "failure_class",
    "latest_resolved_tag",
    "owner_action",
    "proof_artifact",
    "release_tag",
    "repair_command",
    "repository",
    "run_id",
    "snippet_id",
    "url",
}
ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
COMMAND_ALLOWLIST = ("br ", "gh ", "python3 ", "scripts/", "m80 ")
FORBIDDEN_COMMAND_FRAGMENTS = ["\n", "\r", ";", "&&", "||", "|", "`", "$(", ">"]
MUTATING_COMMAND_RE = re.compile(r"(?i)(?:^|[\s/])(?:install|publish|upload|release-artifacts)(?:\s|$)")
MUTABLE_TARGET_RE = re.compile(r"(?i)(?:/releases/latest/|\b(?:main|master|latest)\b|--(?:branch|ref|source)=main)")
PINNED_RELEASE_RE = re.compile(r"(?:\bv[0-9]+\.[0-9]+\.[0-9]+\b|<version>|\{release_tag\}|\{tag\})")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config",
        type=Path,
        default=DEFAULT_CONFIG,
        help="freshness failure policy JSON config",
    )
    parser.add_argument(
        "--emitted-class",
        action="append",
        default=[],
        help="extra verifier-emitted class to require; used by tests and future emitters",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    emitted = set(FRESHNESS_FAILURE_CLASSES) | set(args.emitted_class)
    errors = validate_freshness_failure_policy(
        read_json(args.config),
        source=args.config,
        emitted_classes=emitted,
    )
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"freshness failure policy ok: {args.config}")
    return 0


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise SystemExit(f"freshness failure policy missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: freshness failure policy must be a JSON object")
    return value


def validate_freshness_failure_policy(
    config: dict[str, Any],
    *,
    source: Path | str = "<config>",
    emitted_classes: set[str] | None = None,
) -> list[str]:
    errors: list[str] = []
    emitted = emitted_classes or set(FRESHNESS_FAILURE_CLASSES)
    errors.extend(require_exact_fields(config, TOP_LEVEL_FIELDS, str(source)))
    if config.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"{source}: schema_version must be {SCHEMA_VERSION}")

    classes = config.get("failure_classes")
    if not isinstance(classes, list) or not classes:
        errors.append(f"{source}: failure_classes must be a nonempty list")
        return errors

    seen: set[str] = set()
    present: set[str] = set()
    for index, failure_class in enumerate(classes):
        label = f"{source}:failure_classes[{index}]"
        if not isinstance(failure_class, dict):
            errors.append(f"{label}: failure class must be an object")
            continue
        class_id = failure_class.get("id")
        if isinstance(class_id, str) and class_id:
            label = f"{source}:failure class {class_id}"
            if class_id in seen:
                errors.append(f"{label}: duplicate failure class")
            seen.add(class_id)
            present.add(class_id)
        errors.extend(validate_failure_class(failure_class, label))

    for class_id in sorted(emitted - present):
        errors.append(f"{source}: verifier-emitted class absent from config: {class_id}")
    return errors


def validate_failure_class(value: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(value, CLASS_FIELDS, label))
    class_id = require_nonempty_string(value, "id", label, errors)
    if class_id:
        if ID_RE.fullmatch(class_id) is None:
            errors.append(f"{label}: id must be kebab-case alphanumeric")
        if class_id not in FRESHNESS_FAILURE_CLASSES:
            errors.append(f"{label}: unknown failure class")
    disposition = require_member(value, "disposition", DISPOSITIONS, label, errors)
    require_nonempty_string(value, "description", label, errors)
    require_nonempty_string(value, "owner", label, errors)
    validate_command(value.get("repair_command"), label, errors)
    retry_count = value.get("retry_count")
    if not isinstance(retry_count, int) or retry_count < 0:
        errors.append(f"{label}: retry_count must be a non-negative integer")
    blocks = require_bool(value, "blocks_next_release", label, errors)
    opens_bead = require_bool(value, "opens_bead", label, errors)
    pages = require_bool(value, "pages_maintainer", label, errors)
    manual = require_bool(value, "requires_manual_confirmation", label, errors)
    validate_summary_fields(value.get("safe_summary_fields"), label, errors)

    if disposition == "retry-only":
        if retry_count == 0:
            errors.append(f"{label}: retry-only disposition requires retry_count > 0")
        if any(flag is True for flag in [blocks, opens_bead, pages, manual]):
            errors.append(
                f"{label}: retry-only disposition must not block, page, open beads, or require manual confirmation"
            )
    if disposition == "open-update-bead" and opens_bead is not True:
        errors.append(f"{label}: open-update-bead disposition requires opens_bead=true")
    if disposition == "block-next-release-latest" and blocks is not True:
        errors.append(f"{label}: block-next-release-latest disposition requires blocks_next_release=true")
    if disposition == "page-maintainer" and pages is not True:
        errors.append(f"{label}: page-maintainer disposition requires pages_maintainer=true")
    if disposition == "manual-operator-confirmation" and manual is not True:
        errors.append(
            f"{label}: manual-operator-confirmation disposition requires requires_manual_confirmation=true"
        )
    return errors


def require_exact_fields(value: dict[str, Any], fields: set[str], label: str) -> list[str]:
    errors: list[str] = []
    actual = set(value)
    for missing in sorted(fields - actual):
        errors.append(f"{label}: missing required field {missing}")
    for extra in sorted(actual - fields):
        errors.append(f"{label}: unknown field {extra}")
    return errors


def require_nonempty_string(value: dict[str, Any], field: str, label: str, errors: list[str]) -> str | None:
    observed = value.get(field)
    if not isinstance(observed, str) or not observed.strip():
        errors.append(f"{label}: {field} must be a nonempty string")
        return None
    return observed


def require_member(
    value: dict[str, Any],
    field: str,
    allowed: set[str],
    label: str,
    errors: list[str],
) -> str | None:
    observed = require_nonempty_string(value, field, label, errors)
    if observed is not None and observed not in allowed:
        errors.append(f"{label}: invalid {field}: {observed}")
    return observed


def require_bool(value: dict[str, Any], field: str, label: str, errors: list[str]) -> bool | None:
    observed = value.get(field)
    if not isinstance(observed, bool):
        errors.append(f"{label}: {field} must be a boolean")
        return None
    return observed


def validate_command(value: object, label: str, errors: list[str]) -> None:
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{label}: repair_command must be a nonempty string")
        return
    if not value.startswith(COMMAND_ALLOWLIST):
        errors.append(f"{label}: repair_command starts with unsupported tool")
    for fragment in FORBIDDEN_COMMAND_FRAGMENTS:
        if fragment in value:
            errors.append(f"{label}: repair_command contains {fragment!r}")
    if MUTATING_COMMAND_RE.search(value):
        if MUTABLE_TARGET_RE.search(value):
            errors.append(f"{label}: mutating repair_command must not contain mutable latest/main target")
        if PINNED_RELEASE_RE.search(value) is None:
            errors.append(f"{label}: mutating repair_command must include a concrete or templated release tag")


def validate_summary_fields(value: object, label: str, errors: list[str]) -> None:
    if not isinstance(value, list) or not value:
        errors.append(f"{label}: safe_summary_fields must be a nonempty list")
        return
    seen: set[str] = set()
    for field in value:
        if not isinstance(field, str) or not field:
            errors.append(f"{label}: safe_summary_fields entries must be nonempty strings")
            continue
        if field in seen:
            errors.append(f"{label}: duplicate safe_summary_field: {field}")
        seen.add(field)
        if field not in SAFE_SUMMARY_FIELDS:
            errors.append(f"{label}: unknown safe_summary_field: {field}")


if __name__ == "__main__":
    raise SystemExit(main())
