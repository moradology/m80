#!/usr/bin/env python3
"""Append and verify tamper-evident release proof ledger records."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any
from release_common import require


LEDGER_SCHEMA_VERSION = 2
VERIFIER_RESULT_SCHEMA_VERSION = 1
RECORD_HASH_PREFIX = "sha256:"
LEDGER_NAME = "m80-release-proof-ledger.jsonl"
VERIFIER_NAME = "verify-quickstart-proof.py"
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
    "command",
    "m80_version",
    "bundle_metadata",
    "install",
    "verifier_result",
    "log_artifacts",
    "runner_identity",
    "pass_fail_summary",
    "redaction",
}
VERIFIER_RESULT_FIELDS = {
    "schema_version",
    "verifier",
    "proof_artifact",
    "proof_artifact_digest",
    "release_tag",
    "proof_type",
    "substrate",
    "passed",
    "summary",
    "command",
    "m80_version",
    "bundle_metadata",
    "install",
}
COMMAND_FIELDS = {"display", "expected_exit_status", "observed_exit_status"}
BUNDLE_METADATA_FIELDS = {"path", "digest"}
INSTALL_FIELDS = {"active_pointer", "default_profile"}
FILE_REF_FIELDS = {"path", "digest"}
PASS_FAIL_FIELDS = {"passed", "summary"}
REDACTION_FIELDS = {"host_paths", "secrets", "environment"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)

    append = subcommands.add_parser("append", help="append one proof record")
    add_common_path_args(append)
    append.add_argument("--proof", required=True, type=Path)
    append.add_argument("--verifier-result", required=True, type=Path)
    append.add_argument("--log-artifact", action="append", default=[], type=Path)
    append.add_argument("--workflow-run-id", required=True)
    append.add_argument("--runner-identity", required=True)
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
            verifier_result=args.verifier_result.resolve(),
            log_artifacts=[path.resolve() for path in args.log_artifact],
            workflow_run_id=args.workflow_run_id,
            runner_identity=args.runner_identity,
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
    verifier_result: Path,
    log_artifacts: list[Path],
    workflow_run_id: str,
    runner_identity: str,
    release_tag: str | None,
) -> None:
    require_positive_int_string(workflow_run_id, "workflow-run-id")
    require_nonempty_string(runner_identity, "runner_identity")
    require(log_artifacts, "at least one --log-artifact is required")
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

    verifier_ref = artifact_file_ref(artifact_root, verifier_result, "verifier result")
    verifier_json = read_json(verifier_result, "quickstart verifier result")
    verify_verifier_result(
        verifier_json,
        artifact_root=artifact_root,
        proof_rel=proof_rel,
        proof_digest=sha256_ref(proof),
        proof=proof_json,
        release_tag=proof_release_tag,
        proof_type=proof_type,
        substrate=substrate,
    )
    log_refs = [artifact_file_ref(artifact_root, path, "log artifact") for path in log_artifacts]

    previous_hash = existing[-1]["record_hash"] if existing else None
    record = {
        "schema_version": LEDGER_SCHEMA_VERSION,
        "previous_record_hash": previous_hash,
        "proof_artifact": proof_rel,
        "proof_artifact_digest": sha256_ref(proof),
        "release_tag": proof_release_tag,
        "workflow_run_id": workflow_run_id,
        "proof_type": proof_type,
        "substrate": substrate,
        "command": verifier_json["command"],
        "m80_version": verifier_json["m80_version"],
        "bundle_metadata": verifier_json["bundle_metadata"],
        "install": verifier_json["install"],
        "verifier_result": verifier_ref,
        "log_artifacts": log_refs,
        "runner_identity": runner_identity,
        "pass_fail_summary": {
            "passed": verifier_json["passed"],
            "summary": verifier_json["summary"],
        },
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
    label = f"release proof ledger record {index}"
    require_exact_fields(record, RECORD_FIELDS, label)
    require(record["schema_version"] == LEDGER_SCHEMA_VERSION, f"{label} schema_version mismatch")
    require_hash_ref(record["record_hash"], f"{label} record_hash")
    previous = record["previous_record_hash"]
    require(
        previous is None or (isinstance(previous, str) and HASH_RE.fullmatch(previous) is not None),
        f"{label} previous_record_hash invalid",
    )
    normalize_required_proof(record["proof_artifact"])
    require_hash_ref(record["proof_artifact_digest"], f"{label} proof_artifact_digest")
    require_nonempty_string(record["release_tag"], f"{label} release_tag")
    require_positive_int_string(record["workflow_run_id"], f"{label} workflow_run_id")
    require(record["proof_type"] in PROOF_TYPES, f"{label} proof_type must be hostless or real-kvm")
    require(record["substrate"] in PROOF_TYPES, f"{label} substrate must be hostless or real-kvm")
    verify_command_summary(record["command"], f"{label} command")
    require_nonempty_string(record["m80_version"], f"{label} m80_version")
    verify_bundle_metadata_ref(record["bundle_metadata"], f"{label} bundle_metadata")
    verify_install_summary(record["install"], f"{label} install")
    verify_file_ref_shape(record["verifier_result"], f"{label} verifier_result")
    log_artifacts = record["log_artifacts"]
    require(isinstance(log_artifacts, list) and log_artifacts, f"{label} log_artifacts must be a nonempty list")
    seen_log_paths: set[str] = set()
    for log_index, log_ref in enumerate(log_artifacts, start=1):
        path = verify_file_ref_shape(log_ref, f"{label} log_artifacts {log_index}")
        require(path not in seen_log_paths, f"{label} duplicate log artifact path: {path}")
        seen_log_paths.add(path)
    require_nonempty_string(record["runner_identity"], f"{label} runner_identity")
    verify_pass_fail_summary(record["pass_fail_summary"], f"{label} pass_fail_summary")
    redaction = record["redaction"]
    require(isinstance(redaction, dict), f"{label} redaction must be an object")
    require_exact_fields(redaction, REDACTION_FIELDS, f"{label} redaction")
    for field in REDACTION_FIELDS:
        require(redaction[field] == "omitted", f"{label} redaction {field} must be omitted")


def verify_record_proof(
    record: dict[str, Any],
    *,
    artifact_root: Path,
    release_tag: str | None,
    index: int,
) -> None:
    label = f"release proof ledger record {index}"
    proof_rel = normalize_required_proof(record["proof_artifact"])
    proof_path = artifact_root / proof_rel
    require(proof_path.is_file(), f"{label} proof artifact missing: {proof_rel}")
    require(record["proof_artifact_digest"] == sha256_ref(proof_path), f"{label} proof_artifact_digest mismatch")
    proof = read_json(proof_path, f"{label} proof artifact")
    proof_tag = proof_resolved_tag(proof)
    require(record["release_tag"] == proof_tag, f"{label} release_tag/proof mismatch")
    if release_tag is not None:
        require(
            record["release_tag"] == release_tag,
            f"{label} release_tag mismatch: expected {release_tag}, got {record['release_tag']}",
        )
    require(record["proof_type"] == proof_kind(proof), f"{label} proof_type/proof mismatch")
    require(record["substrate"] == proof_substrate(proof), f"{label} substrate/proof mismatch")
    require(record["command"] == proof_command_summary(proof), f"{label} command/proof mismatch")
    require(record["m80_version"] == proof_m80_version(proof), f"{label} m80_version/proof mismatch")

    bundle_path = artifact_root / normalize_relative_artifact_path(
        record["bundle_metadata"]["path"],
        f"{label} bundle_metadata path",
    )
    require(bundle_path.is_file(), f"{label} bundle metadata missing: {record['bundle_metadata']['path']}")
    require(record["bundle_metadata"]["digest"] == sha256_ref(bundle_path), f"{label} bundle metadata hash mismatch")
    require(
        record["bundle_metadata"]["path"] == proof_bundle_metadata_path(proof),
        f"{label} bundle_metadata/proof mismatch",
    )
    require(record["install"] == proof_install_summary(proof), f"{label} install/proof mismatch")

    verifier_ref = record["verifier_result"]
    verifier_path = artifact_root / normalize_relative_artifact_path(verifier_ref["path"], f"{label} verifier_result path")
    require(verifier_path.is_file(), f"{label} verifier result missing: {verifier_ref['path']}")
    require(verifier_ref["digest"] == sha256_ref(verifier_path), f"{label} verifier result digest mismatch")
    verifier = read_json(verifier_path, f"{label} verifier result")
    verify_verifier_result(
        verifier,
        artifact_root=artifact_root,
        proof_rel=proof_rel,
        proof_digest=record["proof_artifact_digest"],
        proof=proof,
        release_tag=record["release_tag"],
        proof_type=record["proof_type"],
        substrate=record["substrate"],
    )
    require(record["command"] == verifier["command"], f"{label} command/verifier mismatch")
    require(record["m80_version"] == verifier["m80_version"], f"{label} m80_version/verifier mismatch")
    require(record["bundle_metadata"] == verifier["bundle_metadata"], f"{label} bundle_metadata/verifier mismatch")
    require(record["install"] == verifier["install"], f"{label} install/verifier mismatch")
    require(
        record["pass_fail_summary"]["passed"] == verifier["passed"]
        and record["pass_fail_summary"]["summary"] == verifier["summary"],
        f"{label} pass_fail_summary/verifier mismatch",
    )
    for log_ref in record["log_artifacts"]:
        log_path = artifact_root / normalize_relative_artifact_path(log_ref["path"], f"{label} log artifact path")
        require(log_path.is_file(), f"{label} log artifact missing: {log_ref['path']}")
        require(log_ref["digest"] == sha256_ref(log_path), f"{label} log artifact digest mismatch: {log_ref['path']}")


def verify_verifier_result(
    result: dict[str, Any],
    *,
    artifact_root: Path,
    proof_rel: str,
    proof_digest: str,
    proof: dict[str, Any],
    release_tag: str,
    proof_type: str,
    substrate: str,
) -> None:
    require_exact_fields(result, VERIFIER_RESULT_FIELDS, "quickstart verifier result")
    require(
        result["schema_version"] == VERIFIER_RESULT_SCHEMA_VERSION,
        "quickstart verifier result schema_version mismatch",
    )
    require(result["verifier"] == VERIFIER_NAME, "quickstart verifier result verifier mismatch")
    require(result["proof_artifact"] == proof_rel, "quickstart verifier result proof_artifact mismatch")
    require(result["proof_artifact_digest"] == proof_digest, "quickstart verifier result proof_artifact_digest mismatch")
    require(result["release_tag"] == release_tag, "quickstart verifier result release_tag mismatch")
    require(result["proof_type"] == proof_type, "quickstart verifier result proof_type mismatch")
    require(result["substrate"] == substrate, "quickstart verifier result substrate mismatch")
    require(result["passed"] is True, "quickstart verifier result passed must be true")
    require_nonempty_string(result["summary"], "quickstart verifier result summary")
    require(result["command"] == proof_command_summary(proof), "quickstart verifier result command/proof mismatch")
    require(result["m80_version"] == proof_m80_version(proof), "quickstart verifier result m80_version/proof mismatch")
    verify_bundle_metadata_ref(result["bundle_metadata"], "quickstart verifier result bundle_metadata")
    require(
        result["bundle_metadata"]["path"] == proof_bundle_metadata_path(proof),
        "quickstart verifier result bundle_metadata/proof mismatch",
    )
    metadata_path = artifact_root / result["bundle_metadata"]["path"]
    require(metadata_path.is_file(), f"quickstart verifier result bundle metadata missing: {result['bundle_metadata']['path']}")
    require(
        result["bundle_metadata"]["digest"] == sha256_ref(metadata_path),
        "quickstart verifier result bundle metadata hash mismatch",
    )
    require(result["install"] == proof_install_summary(proof), "quickstart verifier result install/proof mismatch")


def record_hash(record: dict[str, Any]) -> str:
    payload = {key: value for key, value in record.items() if key != "record_hash"}
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"{RECORD_HASH_PREFIX}{hashlib.sha256(encoded).hexdigest()}"


def sha256_ref(path: Path) -> str:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return f"{RECORD_HASH_PREFIX}{digest}"


def artifact_file_ref(root: Path, path: Path, label: str) -> dict[str, str]:
    return {
        "path": relative_artifact_name(root, path, label),
        "digest": sha256_ref(path),
    }


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


def proof_command_summary(proof: dict[str, Any]) -> dict[str, Any]:
    command = proof.get("command")
    require(isinstance(command, dict), "quickstart proof command must be an object")
    return {
        "display": require_nonempty_string(command.get("display"), "quickstart proof command display"),
        "expected_exit_status": require_non_negative_int(
            command.get("expected_exit_status"),
            "quickstart proof command expected_exit_status",
        ),
        "observed_exit_status": require_non_negative_int(
            command.get("observed_exit_status"),
            "quickstart proof command observed_exit_status",
        ),
    }


def proof_m80_version(proof: dict[str, Any]) -> str:
    m80 = proof.get("m80")
    require(isinstance(m80, dict), "quickstart proof m80 must be an object")
    return require_nonempty_string(m80.get("version"), "quickstart proof m80 version")


def proof_bundle_metadata_path(proof: dict[str, Any]) -> str:
    bundle = proof.get("bundle")
    require(isinstance(bundle, dict), "quickstart proof bundle must be an object")
    return normalize_relative_artifact_path(bundle.get("metadata_path"), "quickstart proof bundle metadata_path")


def proof_install_summary(proof: dict[str, Any]) -> dict[str, str]:
    install = proof.get("install")
    require(isinstance(install, dict), "quickstart proof install must be an object")
    return {
        "active_pointer": redact_path(install.get("active_pointer"), "quickstart proof install active_pointer"),
        "default_profile": redact_path(install.get("default_profile"), "quickstart proof install default_profile"),
    }


def redact_path(value: object, label: str) -> str:
    path = require_nonempty_string(value, label)
    name = Path(path).name
    require(name not in {"", ".", ".."}, f"{label} must have a file name")
    return f"<redacted>/{name}"


def relative_artifact_name(root: Path, path: Path, label: str) -> str:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise SystemExit(f"{label} must live under artifact-root") from exc
    return normalize_relative_artifact_path(relative.as_posix(), label)


def normalize_required_proof(value: object) -> str:
    return normalize_relative_artifact_path(value, "proof artifact path")


def normalize_relative_artifact_path(value: object, label: str) -> str:
    require(isinstance(value, str) and value, f"{label} must be a nonempty relative path")
    path = Path(value)
    require(not path.is_absolute(), f"{label} must be relative: {value}")
    require(".." not in path.parts, f"{label} must not escape artifact-root: {value}")
    require(path.as_posix() == value, f"{label} must use slash separators: {value}")
    return value


def verify_command_summary(value: object, label: str) -> None:
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, COMMAND_FIELDS, label)
    require_nonempty_string(value["display"], f"{label} display")
    require_non_negative_int(value["expected_exit_status"], f"{label} expected_exit_status")
    require_non_negative_int(value["observed_exit_status"], f"{label} observed_exit_status")


def verify_bundle_metadata_ref(value: object, label: str) -> None:
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, BUNDLE_METADATA_FIELDS, label)
    normalize_relative_artifact_path(value["path"], f"{label} path")
    require_hash_ref(value["digest"], f"{label} digest")


def verify_install_summary(value: object, label: str) -> None:
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, INSTALL_FIELDS, label)
    for field in INSTALL_FIELDS:
        require_redacted_path(value[field], f"{label} {field}")


def verify_file_ref_shape(value: object, label: str) -> str:
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, FILE_REF_FIELDS, label)
    path = normalize_relative_artifact_path(value["path"], f"{label} path")
    require_hash_ref(value["digest"], f"{label} digest")
    return path


def verify_pass_fail_summary(value: object, label: str) -> None:
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, PASS_FAIL_FIELDS, label)
    require(value["passed"] is True, f"{label} passed must be true")
    require_nonempty_string(value["summary"], f"{label} summary")


def require_redacted_path(value: object, label: str) -> None:
    require(
        isinstance(value, str) and value.startswith("<redacted>/") and len(value) > len("<redacted>/"),
        f"{label} must be a redacted path",
    )
    require("/tmp/" not in value and "/home/" not in value and "\\" not in value, f"{label} must not expose host paths")


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


def require_non_negative_int(value: object, label: str) -> int:
    require(isinstance(value, int) and value >= 0, f"{label} must be a nonnegative integer")
    return value


if __name__ == "__main__":
    raise SystemExit(main())
