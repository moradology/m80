#!/usr/bin/env python3
"""Write and validate public-latest freshness drift evidence."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import sys
from typing import Any

from freshness_failure_policy import DEFAULT_CONFIG
from release_freshness import FRESHNESS_FAILURE_CLASSES, classify_freshness_exception
from release_url_contract import public_release_root


SCHEMA_VERSION = 1
VERIFIER_VERSION = "freshness-drift-evidence.v1"
MAX_SAFE_STDERR_EXCERPT_BYTES = 2000
TOP_LEVEL_FIELDS = {
    "schema_version",
    "status",
    "failure_class",
    "repository",
    "workflow_run_id",
    "source",
    "resolved_latest_tag",
    "expected_stable_tag",
    "expected",
    "observed",
    "verifier_version",
    "safe_stderr_excerpt",
    "repair_command",
}
SOURCE_FIELDS = {"kind", "value", "snippet_id"}
EXPECTED_FIELDS = {"asset", "digest", "value"}
OBSERVED_FIELDS = {"status", "digest", "value"}
SOURCE_KINDS = {"file", "url"}
SECRET_PATTERNS = [
    re.compile(r"gh[pousr]_[A-Za-z0-9_]{20,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]+"),
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"(?i)authorization:\s*bearer\s+\S+"),
    re.compile(r"(?i)(?:token|secret|password)=\S+"),
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
]
FIELD_RE = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)=([^;\s]+)")
TAG_SWITCH_RE = re.compile(r"started with (?P<started>v[^,\s]+), guard observed (?P<guard>v[^;\s]+)")
URL_RE = re.compile(r"https?://[^;\s]+")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stderr-file", type=Path, help="stderr captured from scripts/release_freshness.py")
    parser.add_argument("--stdout-file", type=Path, help="stdout captured from scripts/release_freshness.py")
    parser.add_argument("--exit-status", type=int, help="freshness verifier exit status")
    parser.add_argument("--output", type=Path, help="write evidence JSON to this path")
    parser.add_argument("--validate", type=Path, help="validate an existing evidence JSON file")
    parser.add_argument("--workflow-run-id", default=os.environ.get("GITHUB_RUN_ID", "local"))
    parser.add_argument(
        "--repository",
        default=os.environ.get("GITHUB_REPOSITORY", public_release_root().repository),
    )
    parser.add_argument("--expected-stable-tag", help="expected stable tag, if known outside stderr")
    parser.add_argument("--resolved-latest-tag", help="resolved latest tag, if known outside stderr")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.validate is not None:
        evidence = read_json(args.validate)
        errors = validate_evidence(evidence)
        if errors:
            for error in errors:
                print(error, file=sys.stderr)
            return 1
        print(f"freshness drift evidence ok: {args.validate}")
        return 0

    if args.exit_status is None:
        raise SystemExit("--exit-status is required when writing evidence")
    if args.output is None:
        raise SystemExit("--output is required when writing evidence")
    stderr = read_text(args.stderr_file) if args.stderr_file is not None else ""
    stdout = read_text(args.stdout_file) if args.stdout_file is not None else ""
    evidence = build_evidence(
        stderr=stderr,
        stdout=stdout,
        exit_status=args.exit_status,
        repository=args.repository,
        workflow_run_id=args.workflow_run_id,
        expected_stable_tag=args.expected_stable_tag,
        resolved_latest_tag=args.resolved_latest_tag,
    )
    errors = validate_evidence(evidence)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(render_json(evidence))
    print(f"wrote freshness drift evidence: {args.output}")
    return 0


def build_evidence(
    *,
    stderr: str,
    stdout: str,
    exit_status: int,
    repository: str,
    workflow_run_id: str,
    expected_stable_tag: str | None = None,
    resolved_latest_tag: str | None = None,
) -> dict[str, Any]:
    if exit_status == 0:
        return {
            "schema_version": SCHEMA_VERSION,
            "status": "success",
            "failure_class": None,
            "repository": repository,
            "workflow_run_id": workflow_run_id,
            "source": {"kind": "file", "value": "scripts/release_freshness.py", "snippet_id": None},
            "resolved_latest_tag": resolved_latest_tag or resolved_tag_from_success(stdout),
            "expected_stable_tag": expected_stable_tag,
            "expected": {"asset": None, "digest": None, "value": None},
            "observed": {"status": "success", "digest": None, "value": None},
            "verifier_version": VERIFIER_VERSION,
            "safe_stderr_excerpt": safe_excerpt(stderr),
            "repair_command": None,
        }

    fields = parse_fields(stderr)
    failure_class = fields.get("failure_class") or classify_freshness_exception(stderr)
    started, guard = tag_switch_tags(stderr)
    release_tag = fields.get("release_tag")
    resolved_tag = resolved_latest_tag or guard or release_tag
    expected_tag = expected_stable_tag or started or release_tag
    return {
        "schema_version": SCHEMA_VERSION,
        "status": "failure",
        "failure_class": failure_class,
        "repository": repository,
        "workflow_run_id": workflow_run_id,
        "source": source_from_fields(fields, stderr),
        "resolved_latest_tag": resolved_tag,
        "expected_stable_tag": expected_tag,
        "expected": {
            "asset": fields.get("asset"),
            "digest": fields.get("expected_sha256") or fields.get("sha256"),
            "value": expected_value(fields, stderr),
        },
        "observed": {
            "status": fields.get("failure") or fields.get("curl_exit") or failure_class,
            "digest": fields.get("got_sha256"),
            "value": observed_value(fields, stderr),
        },
        "verifier_version": VERIFIER_VERSION,
        "safe_stderr_excerpt": safe_excerpt(stderr),
        "repair_command": repair_command_for(failure_class),
    }


def parse_fields(text: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for match in FIELD_RE.finditer(text):
        fields[match.group(1)] = match.group(2).rstrip(",")
    return fields


def tag_switch_tags(text: str) -> tuple[str | None, str | None]:
    match = TAG_SWITCH_RE.search(text)
    if match is None:
        return None, None
    return match.group("started"), match.group("guard")


def source_from_fields(fields: dict[str, str], stderr: str) -> dict[str, str | None]:
    docs_source = docs_source_from(fields.get("sources"))
    if docs_source is not None:
        return docs_source
    url = fields.get("url") or fields.get("expected_url") or fields.get("got_url")
    if url:
        return {"kind": "url", "value": url, "snippet_id": None}
    match = URL_RE.search(stderr)
    if match is not None:
        return {"kind": "url", "value": match.group(0).rstrip(","), "snippet_id": None}
    return {"kind": "file", "value": "scripts/release_freshness.py", "snippet_id": None}


def docs_source_from(sources: str | None) -> dict[str, str | None] | None:
    if not sources:
        return None
    for source in sources.split(","):
        if not source.startswith("docs:"):
            continue
        parts = source.split(":", 2)
        if len(parts) != 3 or not parts[1] or not parts[2]:
            continue
        _, path, line = parts
        return {
            "kind": "file",
            "value": path,
            "snippet_id": f"{path}:{line}",
        }
    return None


def expected_value(fields: dict[str, str], stderr: str) -> str | None:
    for key in ["expected_url", "expected_size", "url", "release_tag"]:
        if key in fields:
            return fields[key]
    started, _guard = tag_switch_tags(stderr)
    return started


def observed_value(fields: dict[str, str], stderr: str) -> str | None:
    for key in ["got_url", "got_size", "got_sha256", "curl_exit"]:
        if key in fields:
            return fields[key]
    _started, guard = tag_switch_tags(stderr)
    if guard is not None:
        return guard
    return fields.get("failure")


def resolved_tag_from_success(stdout: str) -> str | None:
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError:
        return None
    if not isinstance(payload, dict):
        return None
    value = payload.get("resolved_tag")
    return value if isinstance(value, str) and value else None


def safe_excerpt(stderr: str) -> str:
    excerpt = stderr[-MAX_SAFE_STDERR_EXCERPT_BYTES:]
    for pattern in SECRET_PATTERNS:
        excerpt = pattern.sub("[REDACTED]", excerpt)
    return excerpt


def repair_command_for(failure_class: str | None) -> str | None:
    if not failure_class:
        return None
    try:
        policy = read_json(DEFAULT_CONFIG)
    except SystemExit:
        return None
    for row in policy.get("failure_classes", []):
        if isinstance(row, dict) and row.get("id") == failure_class:
            command = row.get("repair_command")
            return command if isinstance(command, str) else None
    return None


def validate_evidence(value: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    errors.extend(require_exact_fields(value, TOP_LEVEL_FIELDS, "freshness drift evidence"))
    if value.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}")
    status = value.get("status")
    if status not in {"success", "failure"}:
        errors.append("status must be success or failure")
    failure_class = value.get("failure_class")
    if status == "failure":
        if not isinstance(failure_class, str) or not failure_class:
            errors.append("failure artifact must include failure_class")
        elif failure_class not in FRESHNESS_FAILURE_CLASSES:
            errors.append(f"unknown failure_class: {failure_class}")
    if status == "success" and failure_class is not None:
        errors.append("success artifact failure_class must be null")
    require_nonempty_string(value, "repository", "freshness drift evidence", errors)
    require_nonempty_string(value, "workflow_run_id", "freshness drift evidence", errors)
    validate_source(value.get("source"), errors)
    validate_field_object(value.get("expected"), EXPECTED_FIELDS, "expected", errors)
    validate_field_object(value.get("observed"), OBSERVED_FIELDS, "observed", errors)
    if value.get("verifier_version") != VERIFIER_VERSION:
        errors.append(f"verifier_version must be {VERIFIER_VERSION}")
    excerpt = value.get("safe_stderr_excerpt")
    if not isinstance(excerpt, str):
        errors.append("safe_stderr_excerpt must be a string")
    else:
        if len(excerpt.encode()) > MAX_SAFE_STDERR_EXCERPT_BYTES:
            errors.append("safe_stderr_excerpt exceeds 2000 bytes")
        if contains_secret(excerpt):
            errors.append("safe_stderr_excerpt contains an unredacted secret-looking value")
    for field in ["resolved_latest_tag", "expected_stable_tag", "repair_command"]:
        observed = value.get(field)
        if observed is not None and not isinstance(observed, str):
            errors.append(f"{field} must be a string or null")
    return errors


def validate_source(value: object, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append("source must be an object")
        return
    errors.extend(require_exact_fields(value, SOURCE_FIELDS, "source"))
    kind = value.get("kind")
    source_value = value.get("value")
    if kind not in SOURCE_KINDS:
        errors.append("source.kind must be file or url")
    if not isinstance(source_value, str) or not source_value:
        errors.append("source.value must be a nonempty string")
    snippet_id = value.get("snippet_id")
    if snippet_id is not None and (not isinstance(snippet_id, str) or not snippet_id):
        errors.append("source.snippet_id must be a nonempty string or null")


def validate_field_object(value: object, fields: set[str], label: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{label} must be an object")
        return
    errors.extend(require_exact_fields(value, fields, label))
    for field in fields:
        observed = value.get(field)
        if observed is not None and not isinstance(observed, str):
            errors.append(f"{label}.{field} must be a string or null")


def require_exact_fields(value: dict[str, Any], fields: set[str], label: str) -> list[str]:
    errors: list[str] = []
    actual = set(value)
    for missing in sorted(fields - actual):
        errors.append(f"{label}: missing required field {missing}")
    for extra in sorted(actual - fields):
        errors.append(f"{label}: unknown field {extra}")
    return errors


def require_nonempty_string(value: dict[str, Any], field: str, label: str, errors: list[str]) -> None:
    observed = value.get(field)
    if not isinstance(observed, str) or not observed:
        errors.append(f"{label}: {field} must be a nonempty string")


def contains_secret(text: str) -> bool:
    return any(pattern.search(text) for pattern in SECRET_PATTERNS)


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise SystemExit(f"freshness drift evidence missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: freshness drift evidence must be a JSON object")
    return value


def read_text(path: Path | None) -> str:
    if path is None:
        return ""
    try:
        return path.read_text()
    except FileNotFoundError as exc:
        raise SystemExit(f"missing input file: {path}") from exc


def render_json(value: dict[str, Any]) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


if __name__ == "__main__":
    raise SystemExit(main())
