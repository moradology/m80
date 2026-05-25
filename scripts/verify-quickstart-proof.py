#!/usr/bin/env python3
"""Validate m80 quickstart proof artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from quickstart_snippets import quickstart_smoke_argv, quickstart_smoke_command
from release_url_contract import release_asset_url
from release_common import require


SCHEMA_VERSION = 1
TOP_LEVEL_FIELDS = {
    "schema_version",
    "proof_kind",
    "release",
    "command",
    "stream_expectations",
    "stdout",
    "stderr",
    "install",
    "m80",
    "bundle",
    "host_binaries",
    "substrate",
}
RELEASE_FIELDS = {"requested", "resolved_tag", "install_url"}
COMMAND_FIELDS = {"display", "argv", "expected_exit_status", "observed_exit_status", "expected_nonzero"}
STREAM_EXPECTATION_FIELDS = {"stdout_contains", "stderr_contains"}
STREAM_EXCERPT_FIELDS = {"excerpt"}
STREAM_PATH_FIELDS = {"path"}
INSTALL_FIELDS = {"root", "active_pointer", "default_profile"}
M80_FIELDS = {"version", "release_tag", "version_status"}
BUNDLE_FIELDS = {
    "metadata_path",
    "release_tag",
    "m80_version",
    "guest_protocol_version",
    "manifest_schema_version",
}
HOST_BINARY_FIELDS = {"manifest_path", "firecracker_version", "jailer_version"}
SUBSTRATE_FIELDS = {"kind", "summary"}
SUBSTRATE_KINDS = {"hostless", "real-kvm"}
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
VERIFIER_NAME = "verify-quickstart-proof.py"
SHA256_PREFIX = "sha256:"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("proof", type=Path)
    parser.add_argument(
        "--artifact-root",
        type=Path,
        help="directory that contains relative proof artifact paths",
    )
    parser.add_argument("--release-tag", help="expected resolved release tag")
    parser.add_argument(
        "--result-out",
        type=Path,
        help="write a JSON verifier-result artifact after successful validation",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    artifact_root = args.artifact_root or args.proof.parent
    proof = validate_quickstart_proof(args.proof, artifact_root=artifact_root, release_tag=args.release_tag)
    if args.result_out is not None:
        write_verifier_result(
            args.result_out,
            proof=proof,
            proof_path=args.proof.resolve(),
            artifact_root=artifact_root.resolve(),
        )
    print(f"validated quickstart proof: {args.proof}")
    return 0


def validate_quickstart_proof(
    proof_path: Path,
    *,
    artifact_root: Path,
    release_tag: str | None = None,
) -> dict:
    proof = read_json(proof_path, "quickstart proof")
    require_exact_fields(proof, TOP_LEVEL_FIELDS, "quickstart proof")
    require(proof["schema_version"] == SCHEMA_VERSION, "quickstart proof schema_version mismatch")
    require(proof["proof_kind"] in SUBSTRATE_KINDS, "quickstart proof proof_kind must be hostless or real-kvm")

    release = require_object(proof, "release", "quickstart proof")
    require_exact_fields(release, RELEASE_FIELDS, "quickstart proof release")
    resolved_tag = require_nonempty_str(release, "resolved_tag", "quickstart proof release")
    requested = require_nonempty_str(release, "requested", "quickstart proof release")
    require_nonempty_str(release, "install_url", "quickstart proof release")
    require(
        release["install_url"] == release_asset_url(resolved_tag, "install.sh"),
        "quickstart proof release install_url mismatch",
    )
    if release_tag is not None:
        require(
            resolved_tag == release_tag,
            f"quickstart proof resolved tag mismatch: expected {release_tag}, got {resolved_tag}",
        )
    if requested != "latest":
        require(
            requested == resolved_tag,
            f"quickstart proof requested tag mismatch: requested {requested}, resolved {resolved_tag}",
        )

    command = require_object(proof, "command", "quickstart proof")
    require_exact_fields(command, COMMAND_FIELDS, "quickstart proof command")
    require_nonempty_str(command, "display", "quickstart proof command")
    argv = command.get("argv")
    require(
        isinstance(argv, list) and all(isinstance(value, str) and value for value in argv),
        "quickstart proof command argv must be a nonempty string list",
    )
    expected_status = require_int(command, "expected_exit_status", "quickstart proof command")
    observed_status = require_int(command, "observed_exit_status", "quickstart proof command")
    expected_nonzero = command["expected_nonzero"]
    require(isinstance(expected_nonzero, bool), "quickstart proof command expected_nonzero must be a boolean")
    if expected_nonzero:
        require(expected_status != 0, "quickstart proof expected-nonzero command must not expect exit status 0")
        require(
            observed_status == expected_status,
            f"quickstart proof expected-nonzero exit mismatch: expected {expected_status}, got {observed_status}",
        )
    else:
        require(
            command["display"] == quickstart_smoke_command(),
            "quickstart proof normal command display must match the docs quickstart smoke snippet",
        )
        require(
            argv == quickstart_smoke_argv(),
            "quickstart proof normal command argv must match the docs quickstart smoke snippet",
        )
        require(expected_status == 0, "quickstart proof normal command must expect exit status 0")
        require(
            observed_status == 0,
            f"quickstart proof unexpected nonzero exit: expected 0, got {observed_status}",
        )

    expectations = require_object(proof, "stream_expectations", "quickstart proof")
    require_exact_fields(expectations, STREAM_EXPECTATION_FIELDS, "quickstart proof stream_expectations")
    stdout_expected = require_str(expectations, "stdout_contains", "quickstart proof stream_expectations")
    stderr_expected = require_str(expectations, "stderr_contains", "quickstart proof stream_expectations")
    stdout = require_object(proof, "stdout", "quickstart proof")
    require_exact_fields(stdout, STREAM_EXCERPT_FIELDS, "quickstart proof stdout")
    require(isinstance(stdout["excerpt"], str), "quickstart proof stdout excerpt must be a string")
    stderr_text = validate_stderr(proof["stderr"], artifact_root)
    validate_stream_expectations(
        stdout_text=stdout["excerpt"],
        stderr_text=stderr_text,
        stdout_expected=stdout_expected,
        stderr_expected=stderr_expected,
    )

    install = require_object(proof, "install", "quickstart proof")
    require_exact_fields(install, INSTALL_FIELDS, "quickstart proof install")
    for field in INSTALL_FIELDS:
        require_nonempty_str(install, field, "quickstart proof install")

    m80 = require_object(proof, "m80", "quickstart proof")
    require_exact_fields(m80, M80_FIELDS, "quickstart proof m80")
    require_nonempty_str(m80, "version", "quickstart proof m80")
    require(m80["release_tag"] == resolved_tag, "quickstart proof m80 release_tag mismatch")
    require(m80["version_status"] == "release", "quickstart proof m80 version_status must be release")

    bundle = require_object(proof, "bundle", "quickstart proof")
    require_exact_fields(bundle, BUNDLE_FIELDS, "quickstart proof bundle")
    metadata_path = resolve_artifact_path(artifact_root, bundle["metadata_path"], "quickstart proof bundle metadata_path")
    metadata = read_json(metadata_path, "bundle metadata")
    require(metadata.get("release_tag") == resolved_tag, "quickstart proof bundle metadata release_tag mismatch")
    require(bundle["release_tag"] == resolved_tag, "quickstart proof bundle release_tag mismatch")
    require(bundle["m80_version"] == m80["version"], "quickstart proof bundle m80_version mismatch")
    require_positive_int(bundle, "guest_protocol_version", "quickstart proof bundle")
    require_positive_int(bundle, "manifest_schema_version", "quickstart proof bundle")

    host_binaries = require_object(proof, "host_binaries", "quickstart proof")
    require_exact_fields(host_binaries, HOST_BINARY_FIELDS, "quickstart proof host_binaries")
    manifest_path = resolve_artifact_path(
        artifact_root,
        host_binaries["manifest_path"],
        "quickstart proof host_binaries manifest_path",
    )
    read_json(manifest_path, "host-binaries manifest")
    require_nonempty_str(host_binaries, "firecracker_version", "quickstart proof host_binaries")
    require_nonempty_str(host_binaries, "jailer_version", "quickstart proof host_binaries")

    substrate = require_object(proof, "substrate", "quickstart proof")
    require_exact_fields(substrate, SUBSTRATE_FIELDS, "quickstart proof substrate")
    require(substrate["kind"] == proof["proof_kind"], "quickstart proof substrate kind/proof_kind mismatch")
    require(substrate["kind"] in SUBSTRATE_KINDS, "quickstart proof substrate kind must be hostless or real-kvm")
    require_nonempty_str(substrate, "summary", "quickstart proof substrate")
    return proof


def write_verifier_result(
    path: Path,
    *,
    proof: dict,
    proof_path: Path,
    artifact_root: Path,
) -> None:
    bundle = require_object(proof, "bundle", "quickstart proof")
    metadata_path = resolve_artifact_path(
        artifact_root,
        bundle["metadata_path"],
        "quickstart proof bundle metadata_path",
    )
    command = require_object(proof, "command", "quickstart proof")
    install = require_object(proof, "install", "quickstart proof")
    m80 = require_object(proof, "m80", "quickstart proof")
    release = require_object(proof, "release", "quickstart proof")
    substrate = require_object(proof, "substrate", "quickstart proof")
    result = {
        "schema_version": SCHEMA_VERSION,
        "verifier": VERIFIER_NAME,
        "proof_artifact": relative_artifact_path(artifact_root, proof_path, "quickstart proof artifact"),
        "proof_artifact_digest": sha256_ref(proof_path),
        "release_tag": release["resolved_tag"],
        "proof_type": proof["proof_kind"],
        "substrate": substrate["kind"],
        "passed": True,
        "summary": "quickstart proof validated",
        "command": {
            "display": command["display"],
            "expected_exit_status": command["expected_exit_status"],
            "observed_exit_status": command["observed_exit_status"],
        },
        "m80_version": m80["version"],
        "bundle_metadata": {
            "path": bundle["metadata_path"],
            "digest": sha256_ref(metadata_path),
        },
        "install": {
            "active_pointer": redact_path(install["active_pointer"], "quickstart proof install active_pointer"),
            "default_profile": redact_path(install["default_profile"], "quickstart proof install default_profile"),
        },
    }
    require_exact_fields(result, VERIFIER_RESULT_FIELDS, "quickstart verifier result")
    write_json(path, result)


def validate_stderr(stderr: object, artifact_root: Path) -> str:
    require(isinstance(stderr, dict), "quickstart proof stderr must be an object")
    keys = set(stderr)
    if keys == STREAM_EXCERPT_FIELDS:
        require(isinstance(stderr["excerpt"], str), "quickstart proof stderr excerpt must be a string")
        return stderr["excerpt"]
    elif keys == STREAM_PATH_FIELDS:
        return resolve_artifact_path(artifact_root, stderr["path"], "quickstart proof stderr path").read_text()
    else:
        raise SystemExit("quickstart proof stderr must contain exactly path or exactly excerpt")


def validate_stream_expectations(
    *,
    stdout_text: str,
    stderr_text: str,
    stdout_expected: str,
    stderr_expected: str,
) -> None:
    if stdout_expected:
        require(stdout_expected in stdout_text, "quickstart proof stdout missing expected capture")
        require(
            stdout_expected not in stderr_text,
            "quickstart proof stream swap: stdout marker appeared in stderr",
        )
    if stderr_expected:
        require(stderr_expected in stderr_text, "quickstart proof stderr missing expected capture")
        require(
            stderr_expected not in stdout_text,
            "quickstart proof stream swap: stderr marker appeared in stdout",
        )


def resolve_artifact_path(root: Path, value: object, label: str) -> Path:
    require(isinstance(value, str) and value, f"{label} must be a nonempty relative path")
    path = Path(value)
    require(not path.is_absolute(), f"{label} must be relative to the proof artifact root")
    require(".." not in path.parts, f"{label} must not escape the proof artifact root")
    resolved = root / path
    require(resolved.is_file(), f"{label} is missing: {resolved}")
    return resolved


def relative_artifact_path(root: Path, path: Path, label: str) -> str:
    require(path.is_file(), f"{label} is missing: {path}")
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise SystemExit(f"{label} must live under the proof artifact root") from exc
    require(relative.as_posix() == str(relative).replace("\\", "/"), f"{label} must use slash separators")
    require(".." not in relative.parts, f"{label} must not escape the proof artifact root")
    return relative.as_posix()


def sha256_ref(path: Path) -> str:
    return f"{SHA256_PREFIX}{hashlib.sha256(path.read_bytes()).hexdigest()}"


def redact_path(value: object, label: str) -> str:
    require(isinstance(value, str) and value, f"{label} must be a nonempty string")
    name = Path(value).name
    require(name not in {"", ".", ".."}, f"{label} must have a file name")
    return f"<redacted>/{name}"


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    with path.open() as f:
        value = json.load(f)
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    path.chmod(0o644)


def require_object(obj: dict, key: str, label: str) -> dict:
    value = obj.get(key)
    require(isinstance(value, dict), f"{label} {key} must be an object")
    return value


def require_exact_fields(obj: dict, fields: set[str], label: str) -> None:
    actual = set(obj)
    missing = sorted(fields - actual)
    extra = sorted(actual - fields)
    require(not missing, f"{label} missing field(s): {', '.join(missing)}")
    require(not extra, f"{label} has unknown field(s): {', '.join(extra)}")


def require_nonempty_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} {key} must be a nonempty string")
    return value


def require_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str), f"{label} {key} must be a string")
    return value


def require_int(obj: dict, key: str, label: str) -> int:
    value = obj.get(key)
    require(isinstance(value, int), f"{label} {key} must be an integer")
    return value


def require_positive_int(obj: dict, key: str, label: str) -> int:
    value = obj.get(key)
    require(isinstance(value, int) and value > 0, f"{label} {key} must be a positive integer")
    return value


if __name__ == "__main__":
    raise SystemExit(main())
