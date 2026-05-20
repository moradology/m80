#!/usr/bin/env python3
"""Write a hostless quickstart proof fixture for release workflow artifacts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from quickstart_snippets import quickstart_smoke_argv, quickstart_smoke_command
from release_url_contract import release_asset_url


HOSTLESS_SUMMARY = "hostless release fixture; not a real-KVM run-smoke proof"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--artifact-root", required=True, type=Path)
    parser.add_argument("--bundle-metadata", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = args.artifact_root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    metadata = read_json(args.bundle_metadata, "bundle metadata")
    require(metadata.get("release_tag") == args.release_tag, "bundle metadata release_tag mismatch")

    stderr_path = root / "m80-quickstart-stderr.txt"
    stderr_path.write_text("")
    host_manifest_path = root / "m80-quickstart-host-binaries.manifest.json"
    write_json(
        host_manifest_path,
        {
            "schema_version": 1,
            "proof_kind": "hostless-fixture",
            "firecracker_version": "hostless-fixture",
            "jailer_version": "hostless-fixture",
        },
    )

    proof = {
        "schema_version": 1,
        "proof_kind": "hostless",
        "release": {
            "requested": args.release_tag,
            "resolved_tag": args.release_tag,
            "install_url": release_asset_url(args.release_tag, "install.sh"),
        },
        "command": {
            "display": quickstart_smoke_command(),
            "argv": quickstart_smoke_argv(),
            "expected_exit_status": 0,
            "observed_exit_status": 0,
            "expected_nonzero": False,
        },
        "stream_expectations": {
            "stdout_contains": "hello",
            "stderr_contains": "",
        },
        "stdout": {"excerpt": "hello\n"},
        "stderr": {"path": stderr_path.relative_to(root).as_posix()},
        "install": {
            "root": "/tmp/m80-hostless-install-root",
            "active_pointer": "/tmp/m80-hostless-install-root/active",
            "default_profile": "/tmp/m80-hostless-install-root/profiles/default.toml",
        },
        "m80": {
            "version": metadata["m80_version"],
            "release_tag": args.release_tag,
            "version_status": "release",
        },
        "bundle": {
            "metadata_path": args.bundle_metadata.resolve().relative_to(root).as_posix(),
            "release_tag": metadata["release_tag"],
            "m80_version": metadata["m80_version"],
            "guest_protocol_version": metadata["guest_protocol_version"],
            "manifest_schema_version": metadata["manifest_schema_version"],
        },
        "host_binaries": {
            "manifest_path": host_manifest_path.relative_to(root).as_posix(),
            "firecracker_version": "hostless-fixture",
            "jailer_version": "hostless-fixture",
        },
        "substrate": {
            "kind": "hostless",
            "summary": HOSTLESS_SUMMARY,
        },
    }
    write_json(args.out, proof)
    print(args.out)
    return 0


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    with path.open() as f:
        value = json.load(f)
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    path.chmod(0o644)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
