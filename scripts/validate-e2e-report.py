#!/usr/bin/env python3
"""Validate the scripts/run-e2e.sh JSON report contract."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import pathlib
import sys
from collections import Counter
from typing import Any


SCHEMA_VERSION = 2

TOP_LEVEL_FIELDS = {
    "schema_version",
    "generated_at",
    "package",
    "run_root",
    "list_only",
    "timeout_seconds",
    "environment",
    "artifacts",
    "summary",
    "results",
    "exit_code",
}
ENVIRONMENT_FIELDS = {
    "sudo_available",
    "kvm_available",
    "artifacts_available",
    "firecracker_seccomp_filter_available",
    "host_binaries_manifest_available",
    "jailer_harden_available",
    "net_helper_available",
    "iproute2_available",
    "cgroup_v2_available",
    "loop_device_available",
    "debugfs_available",
    "docker_available",
    "measurement_enabled",
    "pmem_artifacts_available",
    "external_network_enabled",
    "malicious_artifacts_available",
    "minimal_artifacts_available",
    "ubuntu_artifacts_available",
}
ARTIFACT_FIELDS = {
    "artifact_dir",
    "kernel",
    "rootfs",
    "firecracker_seccomp_filter",
    "host_binaries_manifest",
    "jailer_harden",
    "net_helper",
}
SUMMARY_FIELDS = {"passed", "failed", "skipped", "listed", "total"}
RESULT_FIELDS = {
    "status",
    "test",
    "binary",
    "reason",
    "exit_code",
    "duration_ms",
    "stdout_excerpt",
    "stderr_excerpt",
}
RESULT_BASE_FIELDS = RESULT_FIELDS - {"stdout_excerpt", "stderr_excerpt"}
RESULT_STATUSES = {"pass", "fail", "skip", "list"}


def _is_nonempty_string(value: Any) -> bool:
    return isinstance(value, str) and bool(value)


def _is_int(value: Any) -> bool:
    return type(value) is int


def _require_exact_fields(
    value: dict[str, Any],
    allowed: set[str],
    *,
    required: set[str],
    prefix: str,
    errors: list[str],
) -> None:
    actual = set(value)
    missing = sorted(required - actual)
    unknown = sorted(actual - allowed)
    if missing:
        errors.append(f"{prefix}: missing field(s): {', '.join(missing)}")
    if unknown:
        errors.append(f"{prefix}: unknown field(s): {', '.join(unknown)}")


def _require_rfc3339_z(value: Any, prefix: str, errors: list[str]) -> None:
    if not isinstance(value, str):
        errors.append(f"{prefix}: must be a string")
        return
    if not value.endswith("Z"):
        errors.append(f"{prefix}: must use UTC Z suffix")
        return
    try:
        dt.datetime.fromisoformat(value.removesuffix("Z") + "+00:00")
    except ValueError:
        errors.append(f"{prefix}: must be RFC3339 timestamp")


def validate_report(report: Any, *, source: str = "<report>") -> list[str]:
    errors: list[str] = []
    if not isinstance(report, dict):
        return [f"{source}: report must be a JSON object"]

    _require_exact_fields(
        report,
        TOP_LEVEL_FIELDS,
        required=TOP_LEVEL_FIELDS,
        prefix=source,
        errors=errors,
    )
    if not _is_int(report.get("schema_version")) or report.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"{source}: schema_version must be {SCHEMA_VERSION}")
    _require_rfc3339_z(report.get("generated_at"), f"{source}.generated_at", errors)

    for field in ("package", "run_root"):
        if not _is_nonempty_string(report.get(field)):
            errors.append(f"{source}.{field}: must be a non-empty string")
    if not isinstance(report.get("list_only"), bool):
        errors.append(f"{source}.list_only: must be boolean")
    if not _is_int(report.get("timeout_seconds")) or report.get("timeout_seconds", 0) <= 0:
        errors.append(f"{source}.timeout_seconds: must be a positive integer")
    if not _is_int(report.get("exit_code")) or report.get("exit_code", -1) < 0:
        errors.append(f"{source}.exit_code: must be a non-negative integer")

    environment = report.get("environment")
    if isinstance(environment, dict):
        _require_exact_fields(
            environment,
            ENVIRONMENT_FIELDS,
            required=ENVIRONMENT_FIELDS,
            prefix=f"{source}.environment",
            errors=errors,
        )
        for field, value in environment.items():
            if field in ENVIRONMENT_FIELDS and not isinstance(value, bool):
                errors.append(f"{source}.environment.{field}: must be boolean")
    else:
        errors.append(f"{source}.environment: must be an object")

    artifacts = report.get("artifacts")
    if isinstance(artifacts, dict):
        _require_exact_fields(
            artifacts,
            ARTIFACT_FIELDS,
            required=ARTIFACT_FIELDS,
            prefix=f"{source}.artifacts",
            errors=errors,
        )
        for field, value in artifacts.items():
            if field in ARTIFACT_FIELDS and not isinstance(value, str):
                errors.append(f"{source}.artifacts.{field}: must be a string")
    else:
        errors.append(f"{source}.artifacts: must be an object")

    summary = report.get("summary")
    if isinstance(summary, dict):
        _require_exact_fields(
            summary,
            SUMMARY_FIELDS,
            required=SUMMARY_FIELDS,
            prefix=f"{source}.summary",
            errors=errors,
        )
        for field, value in summary.items():
            if field in SUMMARY_FIELDS and (not _is_int(value) or value < 0):
                errors.append(f"{source}.summary.{field}: must be a non-negative integer")
    else:
        errors.append(f"{source}.summary: must be an object")

    results = report.get("results")
    if isinstance(results, list):
        counts: Counter[str] = Counter()
        for index, result in enumerate(results):
            result_prefix = f"{source}.results[{index}]"
            if not isinstance(result, dict):
                errors.append(f"{result_prefix}: must be an object")
                continue
            _require_exact_fields(
                result,
                RESULT_FIELDS,
                required=RESULT_BASE_FIELDS,
                prefix=result_prefix,
                errors=errors,
            )
            status = result.get("status")
            if status not in RESULT_STATUSES:
                errors.append(f"{result_prefix}.status: must be one of {sorted(RESULT_STATUSES)}")
                continue
            counts[status] += 1
            if not _is_nonempty_string(result.get("test")):
                errors.append(f"{result_prefix}.test: must be a non-empty string")
            if not _is_nonempty_string(result.get("binary")):
                errors.append(f"{result_prefix}.binary: must be a non-empty string")
            reason = result.get("reason")
            if status == "skip":
                if not _is_nonempty_string(reason):
                    errors.append(f"{result_prefix}.reason: skip results require a reason")
            elif reason is not None and not isinstance(reason, str):
                errors.append(f"{result_prefix}.reason: must be null or string")
            if not _is_int(result.get("exit_code")) or result.get("exit_code", -1) < 0:
                errors.append(f"{result_prefix}.exit_code: must be a non-negative integer")
            if not _is_int(result.get("duration_ms")) or result.get("duration_ms", -1) < 0:
                errors.append(f"{result_prefix}.duration_ms: must be a non-negative integer")
            if status == "fail":
                for excerpt_field in ("stdout_excerpt", "stderr_excerpt"):
                    if not isinstance(result.get(excerpt_field), str):
                        errors.append(f"{result_prefix}.{excerpt_field}: fail results require a string")
            else:
                for excerpt_field in ("stdout_excerpt", "stderr_excerpt"):
                    if excerpt_field in result:
                        errors.append(f"{result_prefix}.{excerpt_field}: only fail results may carry excerpts")
    else:
        errors.append(f"{source}.results: must be an array")
        counts = Counter()

    if isinstance(summary, dict) and isinstance(results, list):
        expected = {
            "passed": counts["pass"],
            "failed": counts["fail"],
            "skipped": counts["skip"],
            "listed": counts["list"],
            "total": len(results),
        }
        for field, expected_value in expected.items():
            if summary.get(field) != expected_value:
                errors.append(
                    f"{source}.summary.{field}: expected {expected_value}, got {summary.get(field)!r}"
                )
    if isinstance(summary, dict) and _is_int(report.get("exit_code")):
        failed = summary.get("failed")
        if failed == 0 and report["exit_code"] != 0:
            errors.append(f"{source}.exit_code: must be 0 when failed summary count is 0")
        if _is_int(failed) and failed > 0 and report["exit_code"] == 0:
            errors.append(f"{source}.exit_code: must be non-zero when failures are present")

    return errors


def read_json(path: pathlib.Path) -> Any:
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=pathlib.Path)
    args = parser.parse_args(argv)

    errors = validate_report(read_json(args.report), source=str(args.report))
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"{args.report}: e2e report schema ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
