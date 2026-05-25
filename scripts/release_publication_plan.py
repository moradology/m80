#!/usr/bin/env python3
"""Plan a fail-closed GitHub release publication step."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import sys
from release_common import parse_timestamp, require


SCHEMA_VERSION = 1
KIND = "m80_release_publication_plan"
ACTIONS = {
    "create_draft_upload_publish",
    "validate_existing_public_release",
    "fail_manual_recovery_required",
}
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"

TOP_LEVEL_FIELDS = {
    "schema_version",
    "kind",
    "release_tag",
    "generated_at",
    "action",
    "release_state",
    "reason",
    "expected_public_asset_count",
    "observed_remote_asset_count",
    "missing_assets",
    "extra_assets",
    "size_mismatches",
    "manual_recovery",
}
PUBLIC_ASSET_FIELDS = {"name", "kind", "sha256", "size_bytes", "integrity_subject"}
SIZE_MISMATCH_FIELDS = {"name", "expected_size_bytes", "observed_size_bytes"}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
DIST_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")
TAG_RE = re.compile(r"^v[0-9]+[.][0-9]+[.][0-9]+$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-dir", required=True, type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--release-metadata", required=True, type=Path)
    parser.add_argument(
        "--manifest",
        type=Path,
        help=f"default: <dist-dir>/{UPLOAD_MANIFEST_NAME}",
    )
    parser.add_argument("--out", type=Path, help="optional publication plan JSON path")
    parser.add_argument("--generated-at", default=None)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    dist_dir = args.dist_dir.resolve()
    manifest_path = (args.manifest or dist_dir / UPLOAD_MANIFEST_NAME).resolve()
    metadata_path = args.release_metadata.resolve()

    manifest = read_json(manifest_path, "release upload manifest")
    metadata = read_json(metadata_path, "GitHub release metadata")
    try:
        plan = build_plan(
            manifest=manifest,
            metadata=metadata,
            release_tag=args.release_tag,
            generated_at=args.generated_at,
        )
        verify_plan(plan, manifest=manifest, metadata=metadata, release_tag=args.release_tag)
    except PlanError as error:
        plan = error.plan
        verify_plan(plan, manifest=manifest, metadata=metadata, release_tag=args.release_tag)
        if args.write and args.out is not None:
            write_json(args.out.resolve(), plan)
        print(error, file=sys.stderr)
        return 1

    if args.write and args.out is not None:
        write_json(args.out.resolve(), plan)
    print(plan["action"])
    return 0


def build_plan(
    *,
    manifest: dict,
    metadata: dict,
    release_tag: str,
    generated_at: str | None,
) -> dict:
    require(TAG_RE.fullmatch(release_tag) is not None, f"release tag must be stable vMAJOR.MINOR.PATCH: {release_tag}")
    manifest_assets = manifest_public_assets(manifest, release_tag)
    expected_names = {asset["name"] for asset in manifest_assets}

    if metadata == {}:
        return plan_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            action="create_draft_upload_publish",
            release_state="absent",
            reason=(
                "GitHub release is absent; create a draft, upload immutable assets, "
                "validate public bytes, then mark latest"
            ),
            expected_public_asset_count=len(manifest_assets),
            observed_remote_asset_count=0,
            missing_assets=[],
            extra_assets=[],
            size_mismatches=[],
            manual_recovery=None,
        )

    remote_assets = metadata_assets(metadata, release_tag)
    observed_names = set(remote_assets)
    missing = sorted(expected_names - observed_names)
    extra = sorted(observed_names - expected_names)
    size_mismatches = [
        {
            "name": asset["name"],
            "expected_size_bytes": asset["size_bytes"],
            "observed_size_bytes": remote_assets[asset["name"]]["size"],
        }
        for asset in sorted(manifest_assets, key=lambda row: row["name"])
        if asset["name"] in remote_assets and asset["size_bytes"] != remote_assets[asset["name"]]["size"]
    ]

    if metadata.get("draft") is True or metadata.get("isDraft") is True:
        fail_plan = plan_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            action="fail_manual_recovery_required",
            release_state="draft",
            reason="draft release already exists; delete the draft release manually before rerun",
            expected_public_asset_count=len(manifest_assets),
            observed_remote_asset_count=len(remote_assets),
            missing_assets=missing,
            extra_assets=extra,
            size_mismatches=size_mismatches,
            manual_recovery=f"gh release delete {release_tag} --yes",
        )
        raise PlanError("draft release exists; delete it manually before rerun", fail_plan)

    if metadata.get("prerelease") is True or metadata.get("isPrerelease") is True:
        fail_plan = plan_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            action="fail_manual_recovery_required",
            release_state="prerelease",
            reason="existing release is marked prerelease; stable latest repair refuses to reuse it",
            expected_public_asset_count=len(manifest_assets),
            observed_remote_asset_count=len(remote_assets),
            missing_assets=missing,
            extra_assets=extra,
            size_mismatches=size_mismatches,
            manual_recovery=f"delete or repair the {release_tag} prerelease outside the protected publish job",
        )
        raise PlanError("existing release is prerelease; refusing stable publish", fail_plan)

    if missing or extra or size_mismatches:
        fail_plan = plan_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            action="fail_manual_recovery_required",
            release_state="public",
            reason="existing public release asset metadata differs from the upload manifest",
            expected_public_asset_count=len(manifest_assets),
            observed_remote_asset_count=len(remote_assets),
            missing_assets=missing,
            extra_assets=extra,
            size_mismatches=size_mismatches,
            manual_recovery=f"delete the bad {release_tag} release or publish a new tag; this job will not clobber public assets",
        )
        raise PlanError("existing public release asset metadata mismatch", fail_plan)

    return plan_payload(
        release_tag=release_tag,
        generated_at=generated_at,
        action="validate_existing_public_release",
        release_state="public",
        reason="public release already exposes the complete asset set; skip upload and validate remote bytes",
        expected_public_asset_count=len(manifest_assets),
        observed_remote_asset_count=len(remote_assets),
        missing_assets=[],
        extra_assets=[],
        size_mismatches=[],
        manual_recovery=None,
    )


def verify_plan(plan: dict, *, manifest: dict, metadata: dict, release_tag: str) -> None:
    require_exact_fields(plan, TOP_LEVEL_FIELDS, "release publication plan")
    require(plan["schema_version"] == SCHEMA_VERSION, "release publication plan schema_version mismatch")
    require(plan["kind"] == KIND, "release publication plan kind mismatch")
    require(plan["release_tag"] == release_tag, "release publication plan release_tag mismatch")
    require(parse_timestamp(plan["generated_at"]) is not None, "release publication plan generated_at invalid")
    require(plan["action"] in ACTIONS, "release publication plan action invalid")
    require(isinstance(plan["release_state"], str) and plan["release_state"], "release publication plan release_state invalid")
    require(isinstance(plan["reason"], str) and plan["reason"], "release publication plan reason invalid")
    require_non_negative_int(plan["expected_public_asset_count"], "release publication plan expected_public_asset_count")
    require_non_negative_int(plan["observed_remote_asset_count"], "release publication plan observed_remote_asset_count")
    require_name_list(plan["missing_assets"], "release publication plan missing_assets")
    require_name_list(plan["extra_assets"], "release publication plan extra_assets")
    verify_size_mismatches(plan["size_mismatches"])
    manual = plan["manual_recovery"]
    require(manual is None or (isinstance(manual, str) and manual.strip()), "release publication plan manual_recovery invalid")

    rebuilt = build_plan_without_raising(manifest=manifest, metadata=metadata, release_tag=release_tag, generated_at=plan["generated_at"])
    require(plan == rebuilt, "release publication plan does not match manifest and GitHub release metadata")


def build_plan_without_raising(*, manifest: dict, metadata: dict, release_tag: str, generated_at: str) -> dict:
    try:
        return build_plan(manifest=manifest, metadata=metadata, release_tag=release_tag, generated_at=generated_at)
    except PlanError as error:
        return error.plan


def plan_payload(
    *,
    release_tag: str,
    generated_at: str | None,
    action: str,
    release_state: str,
    reason: str,
    expected_public_asset_count: int,
    observed_remote_asset_count: int,
    missing_assets: list[str],
    extra_assets: list[str],
    size_mismatches: list[dict],
    manual_recovery: str | None,
) -> dict:
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "release_tag": release_tag,
        "generated_at": generated_at or utc_now(),
        "action": action,
        "release_state": release_state,
        "reason": reason,
        "expected_public_asset_count": expected_public_asset_count,
        "observed_remote_asset_count": observed_remote_asset_count,
        "missing_assets": missing_assets,
        "extra_assets": extra_assets,
        "size_mismatches": size_mismatches,
        "manual_recovery": manual_recovery,
    }


def manifest_public_assets(manifest: dict, release_tag: str) -> list[dict]:
    require(manifest.get("release_tag") == release_tag, "release upload manifest release_tag mismatch")
    assets = require_list(manifest.get("public_assets"), "release upload manifest public_assets")
    result: list[dict] = []
    seen: set[str] = set()
    for row in assets:
        require(isinstance(row, dict), "release upload manifest public asset must be an object")
        require_exact_fields(row, PUBLIC_ASSET_FIELDS, "release upload manifest public asset")
        name = require_dist_name(row["name"], "release upload manifest public asset name")
        require(name not in seen, f"release upload manifest duplicate public asset: {name}")
        seen.add(name)
        require(isinstance(row["kind"], str) and row["kind"], f"release upload manifest public asset {name} kind invalid")
        require_sha256(row["sha256"], f"release upload manifest public asset {name} sha256")
        require_non_negative_int(row["size_bytes"], f"release upload manifest public asset {name} size_bytes")
        require(isinstance(row["integrity_subject"], bool), f"release upload manifest public asset {name} integrity_subject invalid")
        result.append(
            {
                "name": name,
                "kind": row["kind"],
                "sha256": row["sha256"],
                "size_bytes": row["size_bytes"],
            }
        )
    return result


def metadata_assets(metadata: dict, release_tag: str) -> dict[str, dict]:
    tag = metadata.get("tagName", metadata.get("tag_name"))
    require(tag == release_tag, "GitHub release metadata tag mismatch")
    assets = require_list(metadata.get("assets"), "GitHub release metadata assets")
    result: dict[str, dict] = {}
    for row in assets:
        require(isinstance(row, dict), "GitHub release metadata asset must be an object")
        name = require_dist_name(row.get("name"), "GitHub release metadata asset name")
        require(name not in result, f"GitHub release metadata duplicate asset name: {name}")
        size = require_non_negative_int(row.get("size"), f"GitHub release metadata asset {name} size")
        result[name] = {"name": name, "size": size}
    return result


def verify_size_mismatches(value: object) -> None:
    rows = require_list(value, "release publication plan size_mismatches")
    seen: set[str] = set()
    for row in rows:
        require(isinstance(row, dict), "release publication plan size mismatch must be an object")
        require_exact_fields(row, SIZE_MISMATCH_FIELDS, "release publication plan size mismatch")
        name = require_dist_name(row["name"], "release publication plan size mismatch name")
        require(name not in seen, f"release publication plan duplicate size mismatch: {name}")
        seen.add(name)
        require_non_negative_int(
            row["expected_size_bytes"],
            f"release publication plan size mismatch {name} expected_size_bytes",
        )
        require_non_negative_int(
            row["observed_size_bytes"],
            f"release publication plan size mismatch {name} observed_size_bytes",
        )


def require_name_list(value: object, label: str) -> None:
    rows = require_list(value, label)
    seen: set[str] = set()
    for row in rows:
        name = require_dist_name(row, f"{label} entry")
        require(name not in seen, f"{label} duplicate entry: {name}")
        seen.add(name)


def require_list(value: object, label: str) -> list[object]:
    require(isinstance(value, list), f"{label} must be a list")
    return value


def require_dist_name(value: object, label: str) -> str:
    require(
        isinstance(value, str)
        and value
        and value not in {".", ".."}
        and "/" not in value
        and DIST_NAME_RE.fullmatch(value) is not None,
        f"{label} must be a flat dist asset name",
    )
    return value


def require_sha256(value: object, label: str) -> None:
    require(isinstance(value, str) and SHA256_RE.fullmatch(value) is not None, f"{label} must be a lowercase sha256")


def require_non_negative_int(value: object, label: str) -> int:
    require(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")
    return value


def require_exact_fields(obj: dict, expected: set[str], label: str) -> None:
    observed = set(obj)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} field mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def comma_or_none(values: list[str]) -> str:
    return ", ".join(values) if values else "none"


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        payload = json.loads(path.read_text())
    except json.JSONDecodeError as error:
        raise SystemExit(f"{label} is not valid JSON: {path}: {error}") from error
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


class PlanError(Exception):
    def __init__(self, message: str, plan: dict):
        super().__init__(message)
        self.plan = plan


if __name__ == "__main__":
    raise SystemExit(main())
