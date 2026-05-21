#!/usr/bin/env python3
"""Append and verify tamper-evident release proof ledger records."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any


SCHEMA_VERSION = 1
RECORD_HASH_PREFIX = "sha256:"
LEDGER_NAME = "m80-release-proof-ledger.jsonl"
PROOF_TYPES = {"hostless", "real-kvm"}

HASH_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
POSITIVE_INT_RE = re.compile(r"^[1-9][0-9]*$")

RECORD_FIELDS = {
    "schema_version",
    "record_hash",
    "previous_record_hash",
    "proof_artifact",
    "proof_artifact_digest",
    "release_tag",
    "workflow_run_id",
    "proof_type",
    "substrate",
    "redaction",
}
REDACTION_FIELDS = {"host_paths", "secrets", "environment"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)

    append = subcommands.add_parser("append", help="append one proof record")
    add_common_path_args(append)
    append.add_argument("--proof", required=True, type=Path)
    append.add_argument("--workflow-run-id", required=True)
    append.add_argument("--release-tag")

    verify = subcommands.add_parser("verify", help="verify a ledger chain")
    add_common_path_args(verify)
    verify.add_argument("--release-tag")
    verify.add_argument("--require-proof", action="append", default=[])
    verify.add_argument("--require-proof-type", choices=sorted(PROOF_TYPES), action="append", default=[])
    verify.add_argument("--expect-record-count", type=int)
    return parser.parse_args()


def add_common_path_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--ledger",
        required=True,
        type=Path,
        help=f"ledger JSONL path, usually <artifact-root>/{LEDGER_NAME}",
    )
    parser.add_argument(
        "--artifact-root",
        required=True,
        type=Path,
        help="directory that contains proof artifacts named by ledger records",
    )


def main() -> int:
    args = parse_args()
    artifact_root = args.artifact_root.resolve()
    ledger = args.ledger.resolve()
    try:
        ledger.relative_to(artifact_root)
    except ValueError as exc:
        raise SystemExit("ledger must live under artifact-root") from exc

    if args.command == "append":
        append_record(
            ledger=ledger,
            artifact_root=artifact_root,
            proof=args.proof.resolve(),
            workflow_run_id=args.workflow_run_id,
            release_tag=args.release_tag,
        )
    elif args.command == "verify":
        verify_ledger(
            ledger=ledger,
            artifact_root=artifact_root,
            release_tag=args.release_tag,
            required_proofs=args.require_proof,
            required_proof_types=args.require_proof_type,
            expected_record_count=args.expect_record_count,
        )
    else:
        raise AssertionError(args.command)
    return 0


def append_record(
    *,
    ledger: Path,
    artifact_root: Path,
    proof: Path,
    workflow_run_id: str,
    release_tag: str | None,
) -> None:
    require_positive_int_string(workflow_run_id, "workflow-run-id")
    existing = verify_ledger(
        ledger=ledger,
        artifact_root=artifact_root,
        release_tag=None,
        required_proofs=[],
        required_proof_types=[],
        expected_record_count=None,
        allow_missing=True,
    )
    proof_rel = relative_artifact_name(artifact_root, proof, "proof artifact")
    proof_json = read_json(proof, "quickstart proof")
    proof_release_tag = proof_resolved_tag(proof_json)
    if release_tag is not None:
        require(
            proof_release_tag == release_tag,
            f"quickstart proof release tag mismatch: expected {release_tag}, got {proof_release_tag}",
        )
    proof_type = proof_kind(proof_json)
    substrate = proof_substrate(proof_json)
    require(substrate == proof_type, "quickstart proof substrate kind/proof_kind mismatch")

    previous_hash = existing[-1]["record_hash"] if existing else None
    record = {
        "schema_version": SCHEMA_VERSION,
        "previous_record_hash": previous_hash,
        "proof_artifact": proof_rel,
        "proof_artifact_digest": sha256_ref(proof),
        "release_tag": proof_release_tag,
        "workflow_run_id": workflow_run_id,
        "proof_type": proof_type,
        "substrate": substrate,
        "redaction": {
            "host_paths": "omitted",
            "secrets": "omitted",
            "environment": "omitted",
        },
    }
    record["record_hash"] = record_hash(record)
    ledger.parent.mkdir(parents=True, exist_ok=True)
    with ledger.open("a") as f:
        f.write(json.dumps(record, sort_keys=True, separators=(",", ":")))
        f.write("\n")
    ledger.chmod(0o644)
    verify_ledger(
        ledger=ledger,
        artifact_root=artifact_root,
        release_tag=release_tag,
        required_proofs=[proof_rel],
        required_proof_types=[proof_type],
        expected_record_count=None,
    )
    print(f"release proof ledger appended: {ledger} {record['record_hash']}")


def verify_ledger(
    *,
    ledger: Path,
    artifact_root: Path,
    release_tag: str | None,
    required_proofs: list[str],
    required_proof_types: list[str],
    expected_record_count: int | None,
    allow_missing: bool = False,
) -> list[dict[str, Any]]:
    if allow_missing and not ledger.exists():
        return []
    require(ledger.is_file(), f"release proof ledger missing: {ledger}")
    records = read_ledger(ledger)
    require(records, "release proof ledger must contain at least one record")
    if expected_record_count is not None:
        require(expected_record_count >= 0, "expect-record-count must be nonnegative")
        require(
            len(records) == expected_record_count,
            f"release proof ledger record count mismatch: expected {expected_record_count}, got {len(records)}",
        )

    previous_hash: str | None = None
    seen_hashes: set[str] = set()
    seen_proofs: set[str] = set()
    seen_types: set[str] = set()
    for index, record in enumerate(records, start=1):
        verify_record_shape(record, index)
        require(
            record["previous_record_hash"] == previous_hash,
            f"release proof ledger record {index} previous_record_hash mismatch",
        )
        expected_hash = record_hash(record)
        require(
            record["record_hash"] == expected_hash,
            f"release proof ledger record {index} record_hash mismatch",
        )
        require(
            record["record_hash"] not in seen_hashes,
            f"release proof ledger duplicate record_hash: {record['record_hash']}",
        )
        seen_hashes.add(record["record_hash"])
        verify_record_proof(record, artifact_root=artifact_root, release_tag=release_tag, index=index)
        seen_proofs.add(record["proof_artifact"])
        seen_types.add(record["proof_type"])
        previous_hash = record["record_hash"]

    for required in required_proofs:
        required_rel = normalize_required_proof(required)
        require(
            required_rel in seen_proofs,
            f"release proof ledger missing required proof artifact: {required_rel}",
        )
    for required_type in required_proof_types:
        require(
            required_type in seen_types,
            f"release proof ledger missing required proof type: {required_type}",
        )
    print(f"release proof ledger ok: {ledger}")
    return records


def read_ledger(path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for line_number, line in enumerate(path.read_text().splitlines(), start=1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"release proof ledger line {line_number} is not JSON: {exc}") from exc
        require(isinstance(value, dict), f"release proof ledger line {line_number} must be a JSON object")
        records.append(value)
    return records


def verify_record_shape(record: dict[str, Any], index: int) -> None:
    require_exact_fields(record, RECORD_FIELDS, f"release proof ledger record {index}")
    require(
        record["schema_version"] == SCHEMA_VERSION,
        f"release proof ledger record {index} schema_version mismatch",
    )
    require_hash_ref(record["record_hash"], f"release proof ledger record {index} record_hash")
    previous = record["previous_record_hash"]
    require(
        previous is None or (isinstance(previous, str) and HASH_RE.fullmatch(previous) is not None),
        f"release proof ledger record {index} previous_record_hash invalid",
    )
    normalize_required_proof(record["proof_artifact"])
    require_hash_ref(record["proof_artifact_digest"], f"release proof ledger record {index} proof_artifact_digest")
    require_nonempty_string(record["release_tag"], f"release proof ledger record {index} release_tag")
    require_positive_int_string(record["workflow_run_id"], f"release proof ledger record {index} workflow_run_id")
    require(
        record["proof_type"] in PROOF_TYPES,
        f"release proof ledger record {index} proof_type must be hostless or real-kvm",
    )
    require(
        record["substrate"] in PROOF_TYPES,
        f"release proof ledger record {index} substrate must be hostless or real-kvm",
    )
    redaction = record["redaction"]
    require(isinstance(redaction, dict), f"release proof ledger record {index} redaction must be an object")
    require_exact_fields(redaction, REDACTION_FIELDS, f"release proof ledger record {index} redaction")
    for field in REDACTION_FIELDS:
        require(
            redaction[field] == "omitted",
            f"release proof ledger record {index} redaction {field} must be omitted",
        )


def verify_record_proof(
    record: dict[str, Any],
    *,
    artifact_root: Path,
    release_tag: str | None,
    index: int,
) -> None:
    proof_rel = normalize_required_proof(record["proof_artifact"])
    proof_path = artifact_root / proof_rel
    require(proof_path.is_file(), f"release proof ledger record {index} proof artifact missing: {proof_rel}")
    require(
        record["proof_artifact_digest"] == sha256_ref(proof_path),
        f"release proof ledger record {index} proof_artifact_digest mismatch",
    )
    proof = read_json(proof_path, f"release proof ledger record {index} proof artifact")
    proof_tag = proof_resolved_tag(proof)
    require(
        record["release_tag"] == proof_tag,
        f"release proof ledger record {index} release_tag/proof mismatch",
    )
    if release_tag is not None:
        require(
            record["release_tag"] == release_tag,
            f"release proof ledger record {index} release_tag mismatch: expected {release_tag}, got {record['release_tag']}",
        )
    require(
        record["proof_type"] == proof_kind(proof),
        f"release proof ledger record {index} proof_type/proof mismatch",
    )
    require(
        record["substrate"] == proof_substrate(proof),
        f"release proof ledger record {index} substrate/proof mismatch",
    )


def record_hash(record: dict[str, Any]) -> str:
    payload = {key: value for key, value in record.items() if key != "record_hash"}
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"{RECORD_HASH_PREFIX}{hashlib.sha256(encoded).hexdigest()}"


def sha256_ref(path: Path) -> str:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return f"{RECORD_HASH_PREFIX}{digest}"


def proof_resolved_tag(proof: dict[str, Any]) -> str:
    release = proof.get("release")
    require(isinstance(release, dict), "quickstart proof release must be an object")
    return require_nonempty_string(release.get("resolved_tag"), "quickstart proof release resolved_tag")


def proof_kind(proof: dict[str, Any]) -> str:
    value = require_nonempty_string(proof.get("proof_kind"), "quickstart proof proof_kind")
    require(value in PROOF_TYPES, "quickstart proof proof_kind must be hostless or real-kvm")
    return value


def proof_substrate(proof: dict[str, Any]) -> str:
    substrate = proof.get("substrate")
    require(isinstance(substrate, dict), "quickstart proof substrate must be an object")
    value = require_nonempty_string(substrate.get("kind"), "quickstart proof substrate kind")
    require(value in PROOF_TYPES, "quickstart proof substrate kind must be hostless or real-kvm")
    return value


def relative_artifact_name(root: Path, path: Path, label: str) -> str:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise SystemExit(f"{label} must live under artifact-root") from exc
    return normalize_required_proof(relative.as_posix())


def normalize_required_proof(value: object) -> str:
    require(isinstance(value, str) and value, "proof artifact path must be a nonempty relative path")
    path = Path(value)
    require(not path.is_absolute(), f"proof artifact path must be relative: {value}")
    require(".." not in path.parts, f"proof artifact path must not escape artifact-root: {value}")
    require(path.as_posix() == value, f"proof artifact path must use slash separators: {value}")
    return value


def read_json(path: Path, label: str) -> dict[str, Any]:
    require(path.is_file(), f"{label} missing: {path}")
    with path.open() as f:
        value = json.load(f)
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def require_exact_fields(obj: dict[str, Any], fields: set[str], label: str) -> None:
    actual = set(obj)
    missing = sorted(fields - actual)
    extra = sorted(actual - fields)
    require(not missing, f"{label} missing field(s): {', '.join(missing)}")
    require(not extra, f"{label} has unknown field(s): {', '.join(extra)}")


def require_hash_ref(value: object, label: str) -> None:
    require(isinstance(value, str) and HASH_RE.fullmatch(value) is not None, f"{label} must be sha256:<64 hex>")


def require_nonempty_string(value: object, label: str) -> str:
    require(isinstance(value, str) and value.strip(), f"{label} must be a nonempty string")
    return value


def require_positive_int_string(value: object, label: str) -> str:
    require(
        isinstance(value, str) and POSITIVE_INT_RE.fullmatch(value) is not None,
        f"{label} must be a positive integer string",
    )
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
