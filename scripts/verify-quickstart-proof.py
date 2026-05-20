#!/usr/bin/env python3
"""Validate m80 quickstart proof artifacts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from release_url_contract import release_asset_url


SCHEMA_VERSION = 1
TOP_LEVEL_FIELDS = {
    "schema_version",
    "proof_kind",
    "release",
    "command",
    "stdout",
    "stderr",
    "install",
    "m80",
    "bundle",
    "host_binaries",
    "substrate",
}
RELEASE_FIELDS = {"requested", "resolved_tag", "install_url"}
COMMAND_FIELDS = {"display", "argv", "exit_status"}
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("proof", type=Path)
    parser.add_argument(
        "--artifact-root",
        type=Path,
        help="directory that contains relative proof artifact paths",
    )
    parser.add_argument("--release-tag", help="expected resolved release tag")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    artifact_root = args.artifact_root or args.proof.parent
    validate_quickstart_proof(args.proof, artifact_root=artifact_root, release_tag=args.release_tag)
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
    require(isinstance(command["exit_status"], int), "quickstart proof command exit_status must be an integer")

    stdout = require_object(proof, "stdout", "quickstart proof")
    require_exact_fields(stdout, STREAM_EXCERPT_FIELDS, "quickstart proof stdout")
    require(isinstance(stdout["excerpt"], str), "quickstart proof stdout excerpt must be a string")
    validate_stderr(proof["stderr"], artifact_root)

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


def validate_stderr(stderr: object, artifact_root: Path) -> None:
    require(isinstance(stderr, dict), "quickstart proof stderr must be an object")
    keys = set(stderr)
    if keys == STREAM_EXCERPT_FIELDS:
        require(isinstance(stderr["excerpt"], str), "quickstart proof stderr excerpt must be a string")
    elif keys == STREAM_PATH_FIELDS:
        resolve_artifact_path(artifact_root, stderr["path"], "quickstart proof stderr path")
    else:
        raise SystemExit("quickstart proof stderr must contain exactly path or exactly excerpt")


def resolve_artifact_path(root: Path, value: object, label: str) -> Path:
    require(isinstance(value, str) and value, f"{label} must be a nonempty relative path")
    path = Path(value)
    require(not path.is_absolute(), f"{label} must be relative to the proof artifact root")
    require(".." not in path.parts, f"{label} must not escape the proof artifact root")
    resolved = root / path
    require(resolved.is_file(), f"{label} is missing: {resolved}")
    return resolved


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    with path.open() as f:
        value = json.load(f)
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


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


def require_positive_int(obj: dict, key: str, label: str) -> int:
    value = obj.get(key)
    require(isinstance(value, int) and value > 0, f"{label} {key} must be a positive integer")
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
