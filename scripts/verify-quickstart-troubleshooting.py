#!/usr/bin/env python3
"""Validate the public quickstart troubleshooting taxonomy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

from quickstart_snippets import validate_public_command_urls


DEFAULT_MATRIX = Path("docs/behaviors/release/quickstart-troubleshooting-matrix.json")
README = Path("README.md")
RUNBOOK = Path("docs/runbook/release.md")
SCHEMA_VERSION = 1
TOP_LEVEL_FIELDS = {"schema_version", "stable_id_policy", "rows"}
ROW_FIELDS = {
    "id",
    "title",
    "owner_action",
    "action_kind",
    "likely_failing_command",
    "diagnostic_command",
    "repair_command",
    "source_mappings",
    "verifier_codes",
}
SOURCE_FIELDS = {"kind", "ref"}
REQUIRED_IDS = {
    "network",
    "github-auth-rate-limit",
    "missing-asset",
    "checksum-provenance-mismatch",
    "unsupported-tuple",
    "missing-local-tool",
    "kvm-unavailable",
    "host-prerequisite",
    "privilege-denied",
    "stale-profile",
    "process-smoke-failed",
    "unknown-report",
}
OWNER_ACTIONS = {"operator", "maintainer"}
ACTION_KINDS = {"retry", "install", "reinstall", "rollback", "report-release-bug"}
SOURCE_KINDS = {"test", "doc", "source", "proof"}
FAMILIES = {"asset_index", "error_envelope", "host_prerequisite", "install_state"}
ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
COMMAND_RE = re.compile(r"^(command|curl|env|gh|m80|sudo|uname)(?:\s|$)")
ENUM_RE_TEMPLATE = r"enum\s+{name}\s*\{{(?P<body>.*?)\n\}}"
VARIANT_RE = re.compile(r"^\s*([A-Z][A-Za-z0-9_]*)\s*(?:,|=>)", re.MULTILINE)
AS_STR_RE = re.compile(r'Self::[A-Za-z0-9_]+\s*=>\s*"([a-z0-9_]+)"')


ENUM_SOURCES = {
    "asset_index": (
        Path("crates/m80-cli/src/release_asset_index/structured.rs"),
        "AssetIndexDiagnosticCode",
        "as_str",
    ),
    "error_envelope": (
        Path("crates/m80-cli/src/errors.rs"),
        "ErrorDiagnosticCode",
        "serde",
    ),
    "host_prerequisite": (
        Path("crates/m80-preflight/src/host_prerequisite_result/failure.rs"),
        "HostPrerequisiteFailureKind",
        "serde",
    ),
    "install_state": (
        Path("crates/m80-cli/src/install_state.rs"),
        "InstallStateDiagnosticCode",
        "serde",
    ),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    errors = validate_matrix(read_json(args.matrix), source=args.matrix)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"quickstart troubleshooting matrix ok: {args.matrix}")
    return 0


def read_json(path: Path) -> dict[str, Any]:
    try:
        with path.open() as f:
            value = json.load(f)
    except FileNotFoundError as exc:
        raise SystemExit(f"quickstart troubleshooting matrix missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: matrix must be a JSON object")
    return value


def validate_matrix(matrix: dict[str, Any], *, source: Path | str = "<matrix>") -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(matrix, TOP_LEVEL_FIELDS, str(source)))
    if matrix.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"{source}: schema_version must be {SCHEMA_VERSION}")
    if not nonempty_str(matrix.get("stable_id_policy")):
        errors.append(f"{source}: stable_id_policy must be nonempty")

    rows = matrix.get("rows")
    if not isinstance(rows, list) or not rows:
        errors.append(f"{source}: rows must be a nonempty list")
        return errors

    row_ids: set[str] = set()
    covered_codes: dict[str, set[str]] = {family: set() for family in FAMILIES}
    for index, row in enumerate(rows):
        label = f"{source}:rows[{index}]"
        if not isinstance(row, dict):
            errors.append(f"{label}: row must be an object")
            continue
        row_id = row.get("id")
        if isinstance(row_id, str) and row_id:
            label = f"{source}:row {row_id}"
            if row_id in row_ids:
                errors.append(f"{label}: duplicate id")
            row_ids.add(row_id)
        errors.extend(validate_row(row, label))
        collect_codes(row.get("verifier_codes"), covered_codes, label, errors)

    for required_id in sorted(REQUIRED_IDS - row_ids):
        errors.append(f"{source}: missing required troubleshooting id: {required_id}")
    errors.extend(validate_code_coverage(covered_codes, source))
    errors.extend(validate_docs_links(row_ids))
    return errors


def validate_row(row: dict[str, Any], label: str) -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(row, ROW_FIELDS, label))
    row_id = require_nonempty_string(row, "id", label, errors)
    if row_id and ID_RE.fullmatch(row_id) is None:
        errors.append(f"{label}: id must be kebab-case alphanumeric")
    require_nonempty_string(row, "title", label, errors)
    require_member(row, "owner_action", OWNER_ACTIONS, label, errors)
    action_kind = require_member(row, "action_kind", ACTION_KINDS, label, errors)
    repair_command = None
    for field in ["likely_failing_command", "diagnostic_command", "repair_command"]:
        command = require_nonempty_string(row, field, label, errors)
        if command and COMMAND_RE.match(command) is None:
            errors.append(f"{label}: {field} must start with a supported command")
        if command:
            try:
                validate_public_command_urls(command, Path(f"{label}.{field}"))
            except ValueError as exc:
                errors.append(f"{label}: {field}: {exc}")
        if field == "repair_command":
            repair_command = command
    if action_kind == "report-release-bug" and repair_command and "install.sh" in repair_command:
        errors.append(f"{label}: report-release-bug rows must not use an install command")
    errors.extend(validate_source_mappings(row.get("source_mappings"), label))
    if not isinstance(row.get("verifier_codes"), dict):
        errors.append(f"{label}: verifier_codes must be an object")
    return errors


def validate_source_mappings(value: object, label: str) -> list[str]:
    if not isinstance(value, list) or not value:
        return [f"{label}: source_mappings must be a nonempty list"]
    errors: list[str] = []
    for index, item in enumerate(value):
        item_label = f"{label}:source_mappings[{index}]"
        if not isinstance(item, dict):
            errors.append(f"{item_label}: source mapping must be an object")
            continue
        errors.extend(require_exact_fields(item, SOURCE_FIELDS, item_label))
        kind = require_member(item, "kind", SOURCE_KINDS, item_label, errors)
        ref = require_nonempty_string(item, "ref", item_label, errors)
        if kind and ref:
            errors.extend(validate_source_ref(kind, ref, item_label))
    return errors


def validate_source_ref(kind: str, ref: str, label: str) -> list[str]:
    path_text, _, symbol = ref.partition("::")
    path = Path(path_text)
    if not path.exists():
        return [f"{label}: source ref path does not exist: {path_text}"]
    if kind == "test" and not symbol:
        return [f"{label}: test source ref must name a test symbol"]
    if symbol and symbol not in path.read_text():
        return [f"{label}: source ref symbol not found: {symbol}"]
    return []


def collect_codes(
    value: object,
    covered_codes: dict[str, set[str]],
    label: str,
    errors: list[str],
) -> None:
    if not isinstance(value, dict):
        return
    unknown = sorted(set(value) - FAMILIES)
    for family in unknown:
        errors.append(f"{label}: unknown verifier code family: {family}")
    for family, codes in value.items():
        if family not in FAMILIES:
            continue
        if not isinstance(codes, list):
            errors.append(f"{label}: verifier_codes.{family} must be a list")
            continue
        for code in codes:
            if not nonempty_str(code):
                errors.append(f"{label}: verifier_codes.{family} contains a non-string code")
                continue
            covered_codes[family].add(code)


def validate_code_coverage(covered: dict[str, set[str]], source: Path | str) -> list[str]:
    errors: list[str] = []
    for family, required in required_codes().items():
        missing = sorted(required - covered[family])
        unknown = sorted(covered[family] - required)
        for code in missing:
            errors.append(f"{source}: undocumented {family} verifier code: {code}")
        for code in unknown:
            errors.append(f"{source}: unknown {family} verifier code in matrix: {code}")
    return errors


def required_codes() -> dict[str, set[str]]:
    return {
        family: load_enum_codes(path, enum_name, mode)
        for family, (path, enum_name, mode) in ENUM_SOURCES.items()
    }


def load_enum_codes(path: Path, enum_name: str, mode: str) -> set[str]:
    text = path.read_text()
    if mode == "as_str":
        return set(AS_STR_RE.findall(text))
    match = re.search(ENUM_RE_TEMPLATE.format(name=re.escape(enum_name)), text, re.S)
    if match is None:
        raise SystemExit(f"{path}: enum {enum_name} not found")
    variants = VARIANT_RE.findall(match.group("body"))
    return {camel_to_snake(variant) for variant in variants}


def camel_to_snake(value: str) -> str:
    out: list[str] = []
    for index, char in enumerate(value):
        if char.isupper() and index > 0:
            previous = value[index - 1]
            next_char = value[index + 1] if index + 1 < len(value) else ""
            if previous.islower() or previous.isdigit() or next_char.islower():
                out.append("_")
        out.append(char.lower())
    return "".join(out)


def validate_docs_links(row_ids: set[str]) -> list[str]:
    errors: list[str] = []
    targets = [README, RUNBOOK]
    for path in targets:
        text = path.read_text()
        if "quickstart-troubleshooting-matrix.md" not in text:
            errors.append(f"{path}: missing quickstart troubleshooting matrix link")
        for row_id in sorted(row_ids):
            if f"quickstart-troubleshooting-matrix.md#{row_id}" in text:
                break
        else:
            errors.append(f"{path}: matrix link must point at a stable troubleshooting id anchor")
    doc = Path("docs/behaviors/release/quickstart-troubleshooting-matrix.md")
    if not doc.exists():
        errors.append(f"{doc}: missing rendered matrix doc")
    else:
        text = doc.read_text()
        for row_id in sorted(row_ids):
            if f'id="{row_id}"' not in text and f"## {row_id}" not in text:
                errors.append(f"{doc}: missing anchor for troubleshooting id {row_id}")
    return errors


def require_exact_fields(value: dict[str, Any], expected: set[str], label: str) -> list[str]:
    actual = set(value)
    errors = [f"{label}: missing field {field}" for field in sorted(expected - actual)]
    errors.extend(f"{label}: unknown field {field}" for field in sorted(actual - expected))
    return errors


def require_nonempty_string(
    value: dict[str, Any],
    field: str,
    label: str,
    errors: list[str],
) -> str | None:
    field_value = value.get(field)
    if not nonempty_str(field_value):
        errors.append(f"{label}: {field} must be a nonempty string")
        return None
    return field_value


def require_member(
    value: dict[str, Any],
    field: str,
    allowed: set[str],
    label: str,
    errors: list[str],
) -> str | None:
    field_value = require_nonempty_string(value, field, label, errors)
    if field_value and field_value not in allowed:
        errors.append(f"{label}: unknown {field}: {field_value}")
    return field_value


def nonempty_str(value: object) -> bool:
    return isinstance(value, str) and bool(value.strip())


if __name__ == "__main__":
    raise SystemExit(main())
