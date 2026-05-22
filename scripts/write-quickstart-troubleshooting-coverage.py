#!/usr/bin/env python3
"""Write the quickstart troubleshooting row coverage report."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Any


DEFAULT_MATRIX = Path("docs/behaviors/release/quickstart-troubleshooting-matrix.json")
DEFAULT_REPORT = Path("docs/behaviors/release/quickstart-troubleshooting-coverage.json")
SCHEMA_VERSION = 1
REQUIRED_LANES = {
    "installer_failure",
    "bootstrap_network_failure",
    "checksum_or_provenance_failure",
    "host_prerequisite_failure",
    "stale_profile_failure",
    "process_wrapper_smoke",
}
LANE_BY_ID = {
    "network": ["bootstrap_network_failure"],
    "github-auth-rate-limit": ["installer_failure"],
    "missing-asset": ["installer_failure"],
    "checksum-provenance-mismatch": ["installer_failure", "checksum_or_provenance_failure"],
    "unsupported-tuple": ["installer_failure"],
    "missing-local-tool": ["installer_failure"],
    "kvm-unavailable": ["host_prerequisite_failure"],
    "host-prerequisite": ["host_prerequisite_failure"],
    "privilege-denied": ["host_prerequisite_failure"],
    "stale-profile": ["stale_profile_failure"],
    "process-smoke-failed": ["process_wrapper_smoke"],
    "unknown-report": ["bug_report_path"],
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--check", action="store_true", help="fail when the committed report is stale")
    parser.add_argument("--write", action="store_true", help="write the coverage report instead of printing it")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = build_report(read_json(args.matrix), matrix_path=args.matrix)
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.check:
        try:
            existing = args.report.read_text()
        except FileNotFoundError:
            print(f"quickstart troubleshooting coverage report missing: {args.report}", file=sys.stderr)
            return 1
        if existing != rendered:
            print(f"quickstart troubleshooting coverage report is stale: {args.report}", file=sys.stderr)
            print(f"repair: python3 {Path(__file__).as_posix()} --write", file=sys.stderr)
            return 1
        print(f"quickstart troubleshooting coverage current: {args.report}")
        return 0
    if args.write:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(rendered)
        print(f"wrote quickstart troubleshooting coverage: {args.report}")
        return 0
    print(rendered, end="")
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


def build_report(matrix: dict[str, Any], *, matrix_path: Path) -> dict[str, Any]:
    rows = require_rows(matrix)
    report_rows: list[dict[str, Any]] = []
    lane_index: dict[str, list[str]] = {}
    proof_lanes: list[dict[str, Any]] = []

    for row in rows:
        row_id = require_str(row, "id")
        lanes = LANE_BY_ID.get(row_id, [])
        for lane in lanes:
            lane_index.setdefault(lane, []).append(row_id)
        proof_artifacts = proof_artifacts_for(row)
        if row_id == "process-smoke-failed":
            proof_lanes.append(validate_process_smoke_proof(row, proof_artifacts))
        report_rows.append(
            {
                "id": row_id,
                "coverage_lanes": lanes,
                "source_refs": source_refs(row),
                "proof_artifacts": [str(path) for path in proof_artifacts],
                "verifier_code_families": sorted((row.get("verifier_codes") or {}).keys()),
            }
        )

    missing = sorted(lane for lane in REQUIRED_LANES if not lane_index.get(lane))
    if missing:
        raise SystemExit(f"quickstart troubleshooting coverage missing lane(s): {', '.join(missing)}")

    return {
        "schema_version": SCHEMA_VERSION,
        "matrix": display_path(matrix_path),
        "required_lanes": sorted(REQUIRED_LANES),
        "coverage_lanes": {lane: sorted(ids) for lane, ids in sorted(lane_index.items())},
        "manual_or_real_kvm_lanes": proof_lanes,
        "rows": report_rows,
    }


def require_rows(matrix: dict[str, Any]) -> list[dict[str, Any]]:
    rows = matrix.get("rows")
    if not isinstance(rows, list):
        raise SystemExit("quickstart troubleshooting matrix rows must be a list")
    typed: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            raise SystemExit("quickstart troubleshooting matrix rows must be objects")
        typed.append(row)
    return typed


def source_refs(row: dict[str, Any]) -> list[str]:
    mappings = row.get("source_mappings")
    if not isinstance(mappings, list):
        return []
    refs: list[str] = []
    for item in mappings:
        if not isinstance(item, dict):
            continue
        kind = item.get("kind")
        ref = item.get("ref")
        if isinstance(kind, str) and isinstance(ref, str):
            refs.append(f"{kind}:{ref}")
    return refs


def proof_artifacts_for(row: dict[str, Any]) -> list[Path]:
    artifacts: list[Path] = []
    mappings = row.get("source_mappings")
    if not isinstance(mappings, list):
        return artifacts
    for item in mappings:
        if not isinstance(item, dict):
            continue
        if item.get("kind") != "proof":
            continue
        ref = item.get("ref")
        if isinstance(ref, str):
            artifacts.append(Path(ref))
    return artifacts


def validate_process_smoke_proof(row: dict[str, Any], proof_artifacts: list[Path]) -> dict[str, Any]:
    row_id = require_str(row, "id")
    if not proof_artifacts:
        raise SystemExit(f"{row_id}: process-smoke coverage must cite a proof source mapping")
    proof_path = proof_artifacts[0]
    proof = read_json(proof_path)
    substrate = proof.get("substrate")
    process_smoke = proof.get("process_smoke")
    if not isinstance(substrate, dict) or substrate.get("kind") != "linux-kvm-host":
        raise SystemExit(f"{row_id}: proof artifact must be captured on linux-kvm-host")
    if not isinstance(process_smoke, dict) or process_smoke.get("stdout") != "hello\n":
        raise SystemExit(f"{row_id}: proof artifact must contain process_smoke.stdout == hello")
    if process_smoke.get("exit_code") != 0:
        raise SystemExit(f"{row_id}: proof artifact must contain process_smoke.exit_code == 0")
    return {
        "id": row_id,
        "coverage_lane": "process_wrapper_smoke",
        "proof_artifact": str(proof_path),
        "required_substrate": "linux-kvm-host",
        "observable": "process_smoke.exit_code == 0 and process_smoke.stdout == hello\\n",
    }


def require_str(row: dict[str, Any], field: str) -> str:
    value = row.get(field)
    if not isinstance(value, str) or not value:
        raise SystemExit(f"quickstart troubleshooting row missing string field: {field}")
    return value


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(Path.cwd().resolve()).as_posix()
    except ValueError:
        return path.as_posix()


if __name__ == "__main__":
    raise SystemExit(main())
