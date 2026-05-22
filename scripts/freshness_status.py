#!/usr/bin/env python3
"""Validate public latest freshness status artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any

from quickstart_snippets import public_command_inventory
from release_url_contract import public_release_root
from stable_release_channel import REQUIRED_PUBLIC_ASSETS, public_asset_role


SCHEMA_VERSION = 1
VERIFIER_NAME = "verify-freshness-status.py"
SHA256_PREFIX = "sha256:"
SHA256_REF_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
RFC3339_UTC_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
SOURCE_COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
STABLE_TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+$")
STATUS_VALUES = {"pending", "public_green", "scaffolded", "failed", "stale"}
PROOF_SUBSTRATES = {"public-unauthenticated", "fixture-hostless", "real-kvm", "none"}
PROOF_ARTIFACT_CLASSES = {"public", "fixture", "workflow"}
INSTALL_PROOF_KINDS = {"latest-install", "pinned-install"}
TOP_LEVEL_FIELDS = {
    "schema_version",
    "generated_at",
    "status",
    "owner",
    "repo",
    "resolved_latest_tag",
    "expected_highest_stable_tag",
    "latest_install_url",
    "pinned_install_url",
    "proof_artifacts",
    "proof_substrate",
    "workflow_run_id",
    "source_commit",
    "checked_command_inventory_digest",
    "install_url_proofs",
    "public_assets",
    "safety_floor",
}
PROOF_ARTIFACT_FIELDS = {"kind", "path", "sha256", "artifact_class"}
INSTALL_URL_PROOF_FIELDS = {
    "kind",
    "url",
    "final_url",
    "http_status",
    "unauthenticated",
    "artifact_path",
}
PUBLIC_ASSET_FIELDS = {"name", "role", "url", "sha256", "size_bytes"}
SAFETY_FLOOR_FIELDS = {"schema_version", "published_at", "minimum_safe_tag", "yanked_releases"}
MINIMUM_SAFE_FIELDS = {"tag", "reason", "advisory_url", "issue_id", "replacement_command"}
YANKED_RELEASE_FIELDS = {
    "tag",
    "reason",
    "advisory_url",
    "issue_id",
    "published_at",
    "replacement_command",
    "no_replacement_reason",
}
VERIFIER_RESULT_FIELDS = {
    "schema_version",
    "verifier",
    "status_artifact",
    "status_artifact_digest",
    "status",
    "resolved_latest_tag",
    "expected_highest_stable_tag",
    "checked_command_inventory_digest",
    "passed",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("status", type=Path, help="freshness status JSON artifact")
    parser.add_argument(
        "--artifact-root",
        type=Path,
        help="directory containing relative proof artifact paths; defaults to the status file directory",
    )
    parser.add_argument(
        "--docs-root",
        type=Path,
        default=Path("."),
        help="repository root used to compute the public command inventory digest",
    )
    parser.add_argument("--result-out", type=Path, help="write verifier-result JSON after validation")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    artifact_root = args.artifact_root or args.status.parent
    status = validate_freshness_status(
        args.status,
        artifact_root=artifact_root,
        docs_root=args.docs_root,
    )
    if args.result_out is not None:
        write_verifier_result(
            args.result_out,
            status=status,
            status_path=args.status.resolve(),
            artifact_root=artifact_root.resolve(),
        )
    print(f"validated freshness status: {args.status}")
    return 0


def validate_freshness_status(status_path: Path, *, artifact_root: Path, docs_root: Path) -> dict[str, Any]:
    status = read_json(status_path, "freshness status")
    require_exact_fields(status, TOP_LEVEL_FIELDS, "freshness status")
    require(status["schema_version"] == SCHEMA_VERSION, "freshness status schema_version mismatch")
    require_rfc3339(status, "generated_at", "freshness status")

    status_value = require_nonempty_str(status, "status", "freshness status")
    require(status_value in STATUS_VALUES, f"freshness status unknown status: {status_value}")
    proof_substrate = require_nonempty_str(status, "proof_substrate", "freshness status")
    require(
        proof_substrate in PROOF_SUBSTRATES,
        f"freshness status unknown proof_substrate: {proof_substrate}",
    )

    root = public_release_root()
    require(status["owner"] == root.owner, f"freshness status owner mismatch: expected {root.owner}, got {status['owner']!r}")
    require(status["repo"] == root.repo, f"freshness status repo mismatch: expected {root.repo}, got {status['repo']!r}")

    resolved_tag = require_stable_tag(status, "resolved_latest_tag", "freshness status")
    expected_tag = require_stable_tag(status, "expected_highest_stable_tag", "freshness status")
    require_nonempty_str(status, "workflow_run_id", "freshness status")
    source_commit = require_nonempty_str(status, "source_commit", "freshness status")
    require(
        SOURCE_COMMIT_RE.fullmatch(source_commit) is not None,
        "freshness status source_commit must be a 40-character lowercase hex commit",
    )

    require(
        status["latest_install_url"] == root.latest_install_url,
        "freshness status latest_install_url mismatch",
    )
    require(
        status["pinned_install_url"] == root.pinned_install_url(resolved_tag),
        "freshness status pinned_install_url mismatch",
    )
    expected_inventory_digest = command_inventory_digest(docs_root)
    require(
        status["checked_command_inventory_digest"] == expected_inventory_digest,
        "freshness status checked_command_inventory_digest is stale: "
        f"expected {expected_inventory_digest}, got {status['checked_command_inventory_digest']!r}",
    )

    proof_paths = validate_proof_artifacts(
        status["proof_artifacts"],
        artifact_root=artifact_root,
        status_value=status_value,
        proof_substrate=proof_substrate,
    )
    validate_install_url_proofs(
        status["install_url_proofs"],
        status=status,
        proof_paths=proof_paths,
    )
    validate_public_assets(status["public_assets"], status_value=status_value, resolved_tag=resolved_tag)
    validate_safety_floor(status["safety_floor"])

    if status_value == "public_green":
        require(
            resolved_tag == expected_tag,
            "freshness status public_green requires resolved_latest_tag == expected_highest_stable_tag",
        )
        require(
            proof_substrate == "public-unauthenticated",
            "freshness status public_green requires public-unauthenticated proof_substrate",
        )
    if proof_substrate == "fixture-hostless":
        require(
            status_value == "scaffolded",
            "freshness status fixture-hostless proof can appear only as scaffolded status",
        )
    if status_value == "pending":
        require(
            proof_substrate == "none",
            "freshness status pending requires proof_substrate=none",
        )
        require(
            not status["proof_artifacts"],
            "freshness status pending must not reference proof_artifacts",
        )
        require(
            not status["install_url_proofs"],
            "freshness status pending must not reference install_url_proofs",
        )
        require(
            not status["public_assets"],
            "freshness status pending must not reference public_assets",
        )
    return status


def validate_proof_artifacts(
    value: object,
    *,
    artifact_root: Path,
    status_value: str,
    proof_substrate: str,
) -> set[str]:
    require(isinstance(value, list), "freshness status proof_artifacts must be a list")
    if status_value in {"public_green", "scaffolded"}:
        require(value, f"freshness status {status_value} requires proof_artifacts")

    paths: set[str] = set()
    for index, row in enumerate(value):
        label = f"freshness status proof_artifacts[{index}]"
        require(isinstance(row, dict), f"{label} must be an object")
        require_exact_fields(row, PROOF_ARTIFACT_FIELDS, label)
        require_nonempty_str(row, "kind", label)
        path = require_relative_path(row, "path", label)
        digest = require_sha256_ref(row, "sha256", label)
        artifact_class = require_nonempty_str(row, "artifact_class", label)
        require(
            artifact_class in PROOF_ARTIFACT_CLASSES,
            f"{label} artifact_class must be one of {sorted(PROOF_ARTIFACT_CLASSES)}",
        )
        require(path not in paths, f"freshness status duplicate proof_artifact path: {path}")
        resolved = artifact_root / path
        require(resolved.is_file(), f"{label} path is missing: {resolved}")
        require(sha256_ref(resolved) == digest, f"{label} sha256 mismatch for {path}")
        if artifact_class == "fixture":
            require(
                status_value == "scaffolded",
                "freshness status fixture proof can appear only as scaffolded status",
            )
        if status_value == "public_green":
            require(
                artifact_class == "public",
                "freshness status public_green proof_artifacts must be public artifacts",
            )
        if proof_substrate == "public-unauthenticated":
            require(
                artifact_class == "public",
                "freshness status public-unauthenticated substrate cannot use fixture/workflow artifacts",
            )
        paths.add(path)
    return paths


def validate_install_url_proofs(value: object, *, status: dict[str, Any], proof_paths: set[str]) -> None:
    require(isinstance(value, list), "freshness status install_url_proofs must be a list")
    seen: set[str] = set()
    for index, row in enumerate(value):
        label = f"freshness status install_url_proofs[{index}]"
        require(isinstance(row, dict), f"{label} must be an object")
        require_exact_fields(row, INSTALL_URL_PROOF_FIELDS, label)
        kind = require_nonempty_str(row, "kind", label)
        require(kind in INSTALL_PROOF_KINDS, f"{label} unknown kind: {kind}")
        require(kind not in seen, f"freshness status duplicate install_url_proof kind: {kind}")
        seen.add(kind)
        expected_url = status["latest_install_url"] if kind == "latest-install" else status["pinned_install_url"]
        require(row["url"] == expected_url, f"{label} url mismatch for {kind}")
        require_nonempty_str(row, "final_url", label)
        require(isinstance(row["http_status"], int), f"{label} http_status must be an integer")
        require(isinstance(row["unauthenticated"], bool), f"{label} unauthenticated must be a boolean")
        artifact_path = require_relative_path(row, "artifact_path", label)
        require(
            artifact_path in proof_paths,
            f"{label} artifact_path does not reference proof_artifacts: {artifact_path}",
        )

    if status["status"] == "public_green":
        require(seen == INSTALL_PROOF_KINDS, "freshness status public_green requires latest and pinned install proofs")
        for row in value:
            label = f"freshness status install_url_proofs[{row['kind']}]"
            require(row["http_status"] == 200, f"{label} requires http_status=200")
            require(row["unauthenticated"], f"{label} requires unauthenticated=true")


def validate_public_assets(value: object, *, status_value: str, resolved_tag: str) -> None:
    require(isinstance(value, list), "freshness status public_assets must be a list")
    root = public_release_root()
    previous_name: str | None = None
    seen: set[str] = set()
    for index, row in enumerate(value):
        label = f"freshness status public_assets[{index}]"
        require(isinstance(row, dict), f"{label} must be an object")
        require_exact_fields(row, PUBLIC_ASSET_FIELDS, label)
        name = require_nonempty_str(row, "name", label)
        role = require_nonempty_str(row, "role", label)
        url = require_nonempty_str(row, "url", label)
        require_sha256_hex(row, "sha256", label)
        require(
            row["size_bytes"] is None or (isinstance(row["size_bytes"], int) and row["size_bytes"] >= 0),
            f"{label} size_bytes must be a nonnegative integer or null",
        )
        require(role == public_asset_role(name), f"{label} role mismatch for asset {name}")
        require(
            url == root.asset_url(resolved_tag, name),
            f"{label} url mismatch for asset {name}: expected {root.asset_url(resolved_tag, name)}, got {url}",
        )
        require(name not in seen, f"freshness status duplicate public asset: {name}")
        if previous_name is not None:
            require(
                previous_name < name,
                "freshness status public_assets must be sorted by name for deterministic publication",
            )
        previous_name = name
        seen.add(name)

    if status_value == "public_green":
        require(
            tuple(row["name"] for row in value) == tuple(sorted(REQUIRED_PUBLIC_ASSETS)),
            "freshness status public_green public_assets must contain the complete required public asset set",
        )


def validate_safety_floor(value: object) -> None:
    label = "freshness status safety_floor"
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, SAFETY_FLOOR_FIELDS, label)
    require(value["schema_version"] == 1, f"{label} schema_version mismatch")
    require_rfc3339(value, "published_at", label)
    minimum = value["minimum_safe_tag"]
    if minimum is not None:
        validate_minimum_safe_tag(minimum)
    yanked = value["yanked_releases"]
    require(isinstance(yanked, list), f"{label} yanked_releases must be a list")
    for index, row in enumerate(yanked):
        validate_yanked_release(row, index)


def validate_minimum_safe_tag(value: object) -> None:
    label = "freshness status safety_floor.minimum_safe_tag"
    require(isinstance(value, dict), f"{label} must be an object or null")
    require_exact_fields(value, MINIMUM_SAFE_FIELDS, label)
    require_stable_tag(value, "tag", label)
    require_nonempty_str(value, "reason", label)
    require_policy_ref(value, label)
    command = require_nonempty_str(value, "replacement_command", label)
    require_pinned_install_command(command, f"{label} replacement_command")


def validate_yanked_release(value: object, index: int) -> None:
    label = f"freshness status safety_floor.yanked_releases[{index}]"
    require(isinstance(value, dict), f"{label} must be an object")
    require_exact_fields(value, YANKED_RELEASE_FIELDS, label)
    require_stable_tag(value, "tag", label)
    require_nonempty_str(value, "reason", label)
    require_rfc3339(value, "published_at", label)
    require_policy_ref(value, label)
    replacement = require_optional_nonempty_str(value, "replacement_command", label)
    no_replacement = require_optional_nonempty_str(value, "no_replacement_reason", label)
    require(
        replacement is not None or no_replacement is not None,
        f"{label} requires replacement_command or no_replacement_reason",
    )
    require(
        replacement is None or no_replacement is None,
        f"{label} cannot set both replacement_command and no_replacement_reason",
    )
    if replacement is not None:
        require_pinned_install_command(replacement, f"{label} replacement_command")


def require_policy_ref(obj: dict[str, Any], label: str) -> None:
    advisory_url = require_optional_nonempty_str(obj, "advisory_url", label)
    issue_id = require_optional_nonempty_str(obj, "issue_id", label)
    require(
        advisory_url is not None or issue_id is not None,
        f"{label} requires advisory_url or issue_id",
    )


def command_inventory_digest(root: Path) -> str:
    rows = [
        {
            "path": snippet.path.as_posix(),
            "line": snippet.line,
            "classification": snippet.classification,
            "body": snippet.body,
        }
        for snippet in public_command_inventory(root)
    ]
    payload = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()
    return f"{SHA256_PREFIX}{hashlib.sha256(payload).hexdigest()}"


def write_verifier_result(
    path: Path,
    *,
    status: dict[str, Any],
    status_path: Path,
    artifact_root: Path,
) -> None:
    result = {
        "schema_version": SCHEMA_VERSION,
        "verifier": VERIFIER_NAME,
        "status_artifact": relative_artifact_path(artifact_root, status_path, "freshness status artifact"),
        "status_artifact_digest": sha256_ref(status_path),
        "status": status["status"],
        "resolved_latest_tag": status["resolved_latest_tag"],
        "expected_highest_stable_tag": status["expected_highest_stable_tag"],
        "checked_command_inventory_digest": status["checked_command_inventory_digest"],
        "passed": True,
    }
    require_exact_fields(result, VERIFIER_RESULT_FIELDS, "freshness status verifier result")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    path.chmod(0o644)


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


def require_nonempty_str(obj: dict[str, Any], key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} {key} must be a nonempty string")
    return value


def require_optional_nonempty_str(obj: dict[str, Any], key: str, label: str) -> str | None:
    value = obj.get(key)
    if value is None:
        return None
    require(isinstance(value, str) and value, f"{label} {key} must be null or a nonempty string")
    return value


def require_rfc3339(obj: dict[str, Any], key: str, label: str) -> str:
    value = require_nonempty_str(obj, key, label)
    require(RFC3339_UTC_RE.fullmatch(value) is not None, f"{label} {key} must be RFC3339 UTC seconds")
    return value


def require_stable_tag(obj: dict[str, Any], key: str, label: str) -> str:
    value = require_nonempty_str(obj, key, label)
    require(STABLE_TAG_RE.fullmatch(value) is not None, f"{label} {key} must be a stable vMAJOR.MINOR.PATCH tag")
    return value


def require_sha256_ref(obj: dict[str, Any], key: str, label: str) -> str:
    value = require_nonempty_str(obj, key, label)
    require(SHA256_REF_RE.fullmatch(value) is not None, f"{label} {key} must be a sha256:<hex> digest")
    return value


def require_sha256_hex(obj: dict[str, Any], key: str, label: str) -> str:
    value = require_nonempty_str(obj, key, label)
    require(re.fullmatch(r"[0-9a-f]{64}", value) is not None, f"{label} {key} must be a lowercase sha256 hex digest")
    return value


def require_pinned_install_command(command: str, label: str) -> str:
    root = public_release_root()
    prefix = f"curl -fsSL https://github.com/{root.owner}/{root.repo}/releases/download/"
    suffix = "/install.sh | sudo sh"
    require(
        command.startswith(prefix) and command.endswith(suffix),
        f"{label} must be a pinned install.sh command",
    )
    tag = command[len(prefix) : -len(suffix)]
    require(
        STABLE_TAG_RE.fullmatch(tag) is not None,
        f"{label} must use a stable vMAJOR.MINOR.PATCH tag",
    )
    return command


def require_relative_path(obj: dict[str, Any], key: str, label: str) -> str:
    value = require_nonempty_str(obj, key, label)
    path = Path(value)
    require(not path.is_absolute(), f"{label} {key} must be relative")
    require(".." not in path.parts, f"{label} {key} must not escape the artifact root")
    require(path.as_posix() == value, f"{label} {key} must use slash separators")
    return value


def relative_artifact_path(root: Path, path: Path, label: str) -> str:
    require(path.is_file(), f"{label} is missing: {path}")
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise SystemExit(f"{label} must live under the artifact root") from exc
    require(relative.as_posix() == str(relative).replace("\\", "/"), f"{label} must use slash separators")
    require(".." not in relative.parts, f"{label} must not escape the artifact root")
    return relative.as_posix()


def sha256_ref(path: Path) -> str:
    return f"{SHA256_PREFIX}{hashlib.sha256(path.read_bytes()).hexdigest()}"


def require(condition: object, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
