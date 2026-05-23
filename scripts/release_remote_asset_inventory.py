#!/usr/bin/env python3
"""Write and verify the m80 remote release asset inventory."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re


SCHEMA_VERSION = 1
KIND = "m80_release_remote_asset_inventory"
INVENTORY_NAME = "m80-release-remote-assets.json"
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
BUILD_HANDOFF_NAME = "m80-release-build.json"
PUBLISH_RECEIPT_NAME = "m80-release-publish-decision.json"

TOP_LEVEL_FIELDS = {
    "schema_version",
    "kind",
    "release_tag",
    "release_id",
    "generated_at",
    "assets",
}
ASSET_FIELDS = {
    "id",
    "name",
    "kind",
    "size_bytes",
    "sha256",
    "download_url",
    "created_at",
    "updated_at",
}
ASSET_IDENTITY_FIELDS = {"id", "name", "kind", "size_bytes", "sha256", "download_url"}
ASSET_TIMESTAMP_FIELDS = {"created_at", "updated_at"}
PUBLIC_ASSET_FIELDS = {"name", "kind", "sha256", "size_bytes", "integrity_subject"}
BUILD_HANDOFF_FIELDS = {
    "schema_version",
    "release_tag",
    "source_commit",
    "rust_toolchain",
    "target",
    "target_triples",
    "m80_package_version",
    "image_kind",
    "cargo_lock_sha256",
    "builder_identity",
    "builder_os_image",
    "apt_packages",
    "container_digest",
    "bundle_metadata_name",
    "bundle_metadata_sha256",
}
PUBLISH_RECEIPT_FIELDS = {
    "schema_version",
    "kind",
    "decision",
    "release_tag",
    "commit_sha",
    "workflow_run_id",
    "workflow_run_attempt",
    "actor",
    "repository",
    "github_ref",
    "environment_approval_id",
    "generated_at",
    "artifact_manifest",
    "artifact_manifest_digest",
    "proof_ledger",
    "proof_ledger_digest",
    "token_authority",
    "token_authority_digest",
    "quickstart_proofs",
    "public_assets",
    "failure_reason",
}
FILE_REF_FIELDS = {"name", "sha256", "size_bytes"}

SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
SHA256_REF_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
DIST_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--redownload-dir", required=True, type=Path)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--manifest", type=Path, help=f"default: <redownload-dir>/{UPLOAD_MANIFEST_NAME}")
    parser.add_argument("--release-metadata", required=True, type=Path)
    parser.add_argument(
        "--release-assets-metadata",
        type=Path,
        help="optional paginated GitHub release assets JSON from releases/<id>/assets",
    )
    parser.add_argument("--build-handoff", type=Path, help=f"default: <redownload-dir>/{BUILD_HANDOFF_NAME}")
    parser.add_argument("--publish-receipt", type=Path, help=f"default: <redownload-dir>/{PUBLISH_RECEIPT_NAME}")
    parser.add_argument("--commit-sha", help="release commit expected by the build handoff and publish receipt")
    parser.add_argument(
        "--require-rerun-preflight",
        action="store_true",
        help="compare remote inventory against local manifest, build handoff, and publish receipt",
    )
    parser.add_argument("--inventory", type=Path, help=f"default: <redownload-dir>/{INVENTORY_NAME}")
    parser.add_argument("--generated-at", default=None)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    redownload_dir = args.redownload_dir.resolve()
    manifest_path = (args.manifest or redownload_dir / UPLOAD_MANIFEST_NAME).resolve()
    metadata_path = args.release_metadata.resolve()
    assets_metadata_path = args.release_assets_metadata.resolve() if args.release_assets_metadata else None
    build_handoff_path = (args.build_handoff or redownload_dir / BUILD_HANDOFF_NAME).resolve()
    publish_receipt_path = (args.publish_receipt or redownload_dir / PUBLISH_RECEIPT_NAME).resolve()
    inventory_path = (args.inventory or redownload_dir / INVENTORY_NAME).resolve()

    try:
        if args.write:
            inventory = build_inventory(
                redownload_dir=redownload_dir,
                release_tag=args.release_tag,
                manifest_path=manifest_path,
                metadata_path=metadata_path,
                assets_metadata_path=assets_metadata_path,
                generated_at=args.generated_at,
            )
            write_json(inventory_path, inventory)

        inventory = read_json(inventory_path, "remote release asset inventory")
        verify_inventory(
            inventory,
            redownload_dir=redownload_dir,
            release_tag=args.release_tag,
            manifest_path=manifest_path,
            metadata_path=metadata_path,
            assets_metadata_path=assets_metadata_path,
        )
        rerun_preflight_ok = False
        if args.require_rerun_preflight:
            verify_rerun_preflight(
                inventory,
                redownload_dir=redownload_dir,
                release_tag=args.release_tag,
                commit_sha=args.commit_sha,
                manifest_path=manifest_path,
                build_handoff_path=build_handoff_path,
                publish_receipt_path=publish_receipt_path,
            )
            rerun_preflight_ok = True
    except SystemExit as exc:
        if not args.require_rerun_preflight or isinstance(exc.code, int):
            raise
        raise SystemExit(
            f"{exc}\n"
            + rerun_preflight_recovery(
                release_tag=args.release_tag,
                redownload_dir=redownload_dir,
                manifest_path=manifest_path,
                metadata_path=metadata_path,
                assets_metadata_path=assets_metadata_path,
                publish_receipt_path=publish_receipt_path,
            )
        ) from exc
    if rerun_preflight_ok:
        print("rerun preflight recovery_class=safe_identical_rerun")
        print("rerun preflight recovery_command=none")
    print(f"remote release asset inventory ok: {inventory_path}")
    return 0


def rerun_preflight_recovery(
    *,
    release_tag: str,
    redownload_dir: Path,
    manifest_path: Path,
    metadata_path: Path,
    assets_metadata_path: Path | None,
    publish_receipt_path: Path,
) -> str:
    try:
        manifest = read_json(manifest_path, "release upload manifest")
        manifest_assets = normalized_manifest_assets(manifest, release_tag)
        expected_by_name = {asset["name"]: asset for asset in manifest_assets}
        metadata = read_json(metadata_path, "GitHub release metadata")
        remote_rows = release_assets_payload(metadata, assets_metadata_path)
        remote_by_name, duplicate_details = recovery_remote_assets(remote_rows)
        missing = sorted(set(expected_by_name) - set(remote_by_name))
        extra = sorted(set(remote_by_name) - set(expected_by_name))
        mismatch_details = recovery_mismatches(redownload_dir, expected_by_name, remote_by_name)
        receipt_missing = not publish_receipt_path.is_file()
        if missing and not extra and not duplicate_details and not mismatch_details and not receipt_missing:
            recovery_class = "incomplete_draft_delete_and_rerun"
            command = f"gh release delete {release_tag} --yes && rerun the protected tag workflow"
        else:
            recovery_class = "unsafe_manual_intervention_required"
            command = f"inspect or delete the bad {release_tag} release outside the protected publish job"

        details = [
            f"missing_assets={comma_or_none(missing)}",
            f"extra_assets={comma_or_none(extra)}",
            "remote_assets=" + semicolon_or_none(recovery_asset_details(remote_by_name)),
            "digest_mismatches=" + semicolon_or_none(mismatch_details),
            "duplicate_remote_assets=" + semicolon_or_none(duplicate_details),
            f"publish_receipt_missing={str(receipt_missing).lower()}",
        ]
        return "\n".join(
            [
                f"rerun preflight recovery_class={recovery_class}",
                f"rerun preflight recovery_command={command}",
                "rerun preflight recovery_detail " + " ".join(details),
                f"repair: {command}; the protected publish job will not clobber, overwrite, or trust mismatched public assets",
            ]
        )
    except SystemExit:
        return "\n".join(
            [
                "rerun preflight recovery_class=unsafe_manual_intervention_required",
                f"rerun preflight recovery_command=inspect or delete the bad {release_tag} release outside the protected publish job",
                "rerun preflight recovery_detail unavailable=malformed-release-metadata",
                (
                    f"repair: inspect or delete the bad {release_tag} release outside the protected publish job; "
                    "the protected publish job will not clobber, overwrite, or trust mismatched public assets"
                ),
            ]
        )


def build_inventory(
    *,
    redownload_dir: Path,
    release_tag: str,
    manifest_path: Path,
    metadata_path: Path,
    assets_metadata_path: Path | None,
    generated_at: str | None,
) -> dict:
    manifest_assets = normalized_manifest_assets(read_json(manifest_path, "release upload manifest"), release_tag)
    metadata = read_json(metadata_path, "GitHub release metadata")
    release_id = release_metadata_id(metadata, release_tag)
    remote_assets = release_assets_payload(metadata, assets_metadata_path)
    remote_by_name = remote_assets_by_name(remote_assets, release_tag)
    require_name_set(
        set(remote_by_name),
        {asset["name"] for asset in manifest_assets},
        "remote release asset metadata set",
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "release_tag": release_tag,
        "release_id": release_id,
        "generated_at": generated_at or utc_now(),
        "assets": [
            inventory_asset_row(redownload_dir, manifest_asset, remote_by_name[manifest_asset["name"]], release_tag)
            for manifest_asset in sorted(manifest_assets, key=lambda row: row["name"])
        ],
    }


def verify_inventory(
    inventory: dict,
    *,
    redownload_dir: Path,
    release_tag: str,
    manifest_path: Path,
    metadata_path: Path,
    assets_metadata_path: Path | None,
) -> None:
    require_exact_fields(inventory, TOP_LEVEL_FIELDS, "remote release asset inventory")
    require(inventory["schema_version"] == SCHEMA_VERSION, "remote release asset inventory schema_version mismatch")
    require(inventory["kind"] == KIND, "remote release asset inventory kind mismatch")
    require(inventory["release_tag"] == release_tag, "remote release asset inventory release_tag mismatch")
    require(parse_timestamp(inventory["generated_at"]) is not None, "remote release asset inventory generated_at invalid")

    expected = build_inventory(
        redownload_dir=redownload_dir,
        release_tag=release_tag,
        manifest_path=manifest_path,
        metadata_path=metadata_path,
        assets_metadata_path=assets_metadata_path,
        generated_at=inventory["generated_at"],
    )
    require(inventory["release_id"] == expected["release_id"], "remote release asset inventory release_id mismatch")

    observed_assets = normalized_inventory_assets(inventory["assets"])
    expected_assets = normalized_inventory_assets(expected["assets"])
    observed_identity = asset_identity_rows(observed_assets)
    expected_identity = asset_identity_rows(expected_assets)
    require(
        observed_identity == expected_identity,
        "remote release asset inventory identity mismatch",
    )


def verify_rerun_preflight(
    inventory: dict,
    *,
    redownload_dir: Path,
    release_tag: str,
    commit_sha: str | None,
    manifest_path: Path,
    build_handoff_path: Path,
    publish_receipt_path: Path,
) -> None:
    require(
        isinstance(commit_sha, str) and COMMIT_RE.fullmatch(commit_sha) is not None,
        "rerun preflight commit-sha must be a 40-character lowercase hex commit",
    )
    manifest = read_json(manifest_path, "release upload manifest")
    build_handoff = read_json(build_handoff_path, "release build handoff")
    publish_receipt = read_json(publish_receipt_path, "release publish decision receipt")

    manifest_assets = normalized_manifest_assets(manifest, release_tag)
    inventory_assets = normalized_inventory_assets(inventory["assets"])
    manifest_by_name = {asset["name"]: asset for asset in manifest_assets}
    inventory_by_name = {asset["name"]: asset for asset in inventory_assets}
    require_name_set(set(inventory_by_name), set(manifest_by_name), "rerun preflight remote inventory asset set")

    for name, manifest_asset in sorted(manifest_by_name.items()):
        inventory_asset = inventory_by_name[name]
        for field in ("kind", "sha256", "size_bytes"):
            require(
                inventory_asset[field] == manifest_asset[field],
                f"rerun preflight asset {name} {field} mismatch between remote inventory and upload manifest",
            )

    verify_build_handoff(
        build_handoff,
        redownload_dir=redownload_dir,
        release_tag=release_tag,
        commit_sha=commit_sha,
        inventory_by_name=inventory_by_name,
    )
    verify_publish_receipt(
        publish_receipt,
        manifest_path=manifest_path,
        release_tag=release_tag,
        commit_sha=commit_sha,
        manifest_assets=manifest_assets,
        inventory_by_name=inventory_by_name,
    )


def normalized_manifest_assets(manifest: dict, release_tag: str) -> list[dict]:
    require(manifest.get("release_tag") == release_tag, "release upload manifest release_tag mismatch")
    assets = manifest.get("public_assets")
    require(isinstance(assets, list), "release upload manifest public_assets must be a list")
    result = []
    seen: set[str] = set()
    for row in assets:
        require(isinstance(row, dict), "release upload manifest public asset must be an object")
        require_exact_fields(row, PUBLIC_ASSET_FIELDS, "release upload manifest public asset")
        name = require_dist_name(row["name"], "release upload manifest public asset name")
        require(name not in seen, f"release upload manifest public asset duplicate name: {name}")
        seen.add(name)
        require(isinstance(row["kind"], str) and row["kind"], f"release upload manifest public asset {name} kind invalid")
        require_sha256(row["sha256"], f"release upload manifest public asset {name} sha256")
        require_non_negative_int(row["size_bytes"], f"release upload manifest public asset {name} size_bytes")
        result.append(
            {
                "name": name,
                "kind": row["kind"],
                "sha256": row["sha256"],
                "size_bytes": row["size_bytes"],
            }
        )
    return result


def verify_build_handoff(
    build_handoff: dict,
    *,
    redownload_dir: Path,
    release_tag: str,
    commit_sha: str,
    inventory_by_name: dict[str, dict],
) -> None:
    require_exact_fields(build_handoff, BUILD_HANDOFF_FIELDS, "release build handoff")
    require(build_handoff["schema_version"] == 1, "release build handoff schema_version mismatch")
    require(build_handoff["release_tag"] == release_tag, "release build handoff release_tag mismatch")
    require(build_handoff["source_commit"] == commit_sha, "release build handoff source_commit mismatch")
    metadata_name = require_dist_name(build_handoff["bundle_metadata_name"], "release build handoff bundle_metadata_name")
    require_sha256(build_handoff["bundle_metadata_sha256"], "release build handoff bundle_metadata_sha256")
    require(
        metadata_name in inventory_by_name,
        f"rerun preflight build handoff metadata asset missing from remote inventory: {metadata_name}",
    )
    require(
        inventory_by_name[metadata_name]["sha256"] == build_handoff["bundle_metadata_sha256"],
        f"rerun preflight build handoff metadata digest mismatch for {metadata_name}",
    )
    require_named_file_matches_inventory(
        BUILD_HANDOFF_NAME,
        redownload_dir=redownload_dir,
        inventory_by_name=inventory_by_name,
        label="release build handoff",
    )


def verify_publish_receipt(
    receipt: dict,
    *,
    manifest_path: Path,
    release_tag: str,
    commit_sha: str,
    manifest_assets: list[dict],
    inventory_by_name: dict[str, dict],
) -> None:
    require_exact_fields(receipt, PUBLISH_RECEIPT_FIELDS, "release publish decision receipt")
    require(receipt["schema_version"] == 3, "release publish decision receipt schema_version mismatch")
    require(receipt["kind"] == "m80_release_publish_decision", "release publish decision receipt kind mismatch")
    require(receipt["decision"] == "approved", "release publish decision receipt decision must be approved")
    require(receipt["failure_reason"] is None, "approved release publish decision receipt must not have failure_reason")
    require(receipt["release_tag"] == release_tag, "release publish decision receipt release_tag mismatch")
    require(receipt["commit_sha"] == commit_sha, "release publish decision receipt commit_sha mismatch")
    artifact_manifest = verify_file_ref(
        receipt["artifact_manifest"],
        path=manifest_path,
        expected_name=UPLOAD_MANIFEST_NAME,
        label="release publish decision receipt artifact_manifest",
    )
    require(
        receipt["artifact_manifest_digest"] == artifact_manifest["sha256"],
        "release publish decision receipt artifact_manifest_digest mismatch",
    )
    receipt_assets = normalized_manifest_assets(
        {"release_tag": release_tag, "public_assets": receipt["public_assets"]},
        release_tag,
    )
    require(
        sorted(receipt_assets, key=lambda row: row["name"]) == sorted(manifest_assets, key=lambda row: row["name"]),
        "release publish decision receipt public_assets mismatch upload manifest",
    )
    for asset in receipt_assets:
        remote = inventory_by_name[asset["name"]]
        for field in ("kind", "sha256", "size_bytes"):
            require(
                remote[field] == asset[field],
                f"rerun preflight asset {asset['name']} {field} mismatch between remote inventory and publish receipt",
            )


def verify_file_ref(ref: object, *, path: Path, expected_name: str, label: str) -> dict:
    require(isinstance(ref, dict), f"{label} must be an object")
    require_exact_fields(ref, FILE_REF_FIELDS, label)
    require(ref["name"] == expected_name, f"{label} name mismatch")
    require_sha256_ref(ref["sha256"], f"{label} sha256")
    require_non_negative_int(ref["size_bytes"], f"{label} size_bytes")
    require(path.is_file(), f"{label} missing: {path}")
    expected = file_ref(path)
    require(ref == expected, f"{label} digest mismatch")
    return expected


def require_named_file_matches_inventory(
    name: str,
    *,
    redownload_dir: Path,
    inventory_by_name: dict[str, dict],
    label: str,
) -> None:
    require(name in inventory_by_name, f"rerun preflight {label} asset missing from remote inventory: {name}")
    path = redownload_dir / name
    require(path.is_file(), f"rerun preflight {label} downloaded bytes missing: {name}")
    asset = inventory_by_name[name]
    require(path.stat().st_size == asset["size_bytes"], f"rerun preflight {label} size mismatch: {name}")
    require(sha256_file(path) == asset["sha256"], f"rerun preflight {label} sha256 mismatch: {name}")


def release_assets_payload(metadata: dict, assets_metadata_path: Path | None) -> list[object]:
    if assets_metadata_path is None:
        assets = metadata.get("assets")
        require(isinstance(assets, list), "GitHub release metadata assets must be a list")
        return assets
    payload = read_json_value(assets_metadata_path, "GitHub release assets metadata")
    require(isinstance(payload, list), "GitHub release assets metadata must be a list")
    if not payload or all(isinstance(row, dict) for row in payload):
        return payload
    pages: list[object] = []
    for page in payload:
        require(isinstance(page, list), "GitHub release assets metadata page must be a list")
        pages.extend(page)
    return pages


def remote_assets_by_name(assets: list[object], release_tag: str) -> dict[str, dict]:
    require(isinstance(assets, list), "GitHub release metadata assets must be a list")
    by_name: dict[str, dict] = {}
    ids: set[int] = set()
    for row in assets:
        require(isinstance(row, dict), "GitHub release metadata asset must be an object")
        name = require_dist_name(row.get("name"), "GitHub release metadata asset name")
        require(name not in by_name, f"GitHub release metadata duplicate asset name: {name}")
        asset_id = require_positive_int(row.get("id"), f"GitHub release metadata asset {name} id")
        require(asset_id not in ids, f"GitHub release metadata duplicate asset id: {asset_id}")
        ids.add(asset_id)
        require_non_negative_int(row.get("size"), f"GitHub release metadata asset {name} size")
        download_url = require_download_url(row.get("browser_download_url"), release_tag, name)
        created_at = require_timestamp(row.get("created_at"), f"GitHub release metadata asset {name} created_at")
        updated_at = require_timestamp(row.get("updated_at"), f"GitHub release metadata asset {name} updated_at")
        by_name[name] = {
            "id": asset_id,
            "name": name,
            "size": row["size"],
            "browser_download_url": download_url,
            "created_at": created_at,
            "updated_at": updated_at,
        }
    return by_name


def inventory_asset_row(redownload_dir: Path, manifest_asset: dict, remote_asset: dict, release_tag: str) -> dict:
    name = manifest_asset["name"]
    path = redownload_dir / name
    require(path.is_file(), f"remote release asset downloaded bytes missing: {name}")
    actual_size = path.stat().st_size
    actual_sha = sha256_file(path)
    require(
        actual_size == manifest_asset["size_bytes"],
        f"remote release asset {name} size mismatch: expected {manifest_asset['size_bytes']}, got {actual_size}",
    )
    require(
        remote_asset["size"] == manifest_asset["size_bytes"],
        f"remote release asset {name} GitHub size mismatch: expected {manifest_asset['size_bytes']}, got {remote_asset['size']}",
    )
    require(
        actual_sha == manifest_asset["sha256"],
        f"remote release asset {name} sha256 mismatch: expected {manifest_asset['sha256']}, got {actual_sha}",
    )
    return {
        "id": remote_asset["id"],
        "name": name,
        "kind": manifest_asset["kind"],
        "size_bytes": actual_size,
        "sha256": actual_sha,
        "download_url": require_download_url(remote_asset["browser_download_url"], release_tag, name),
        "created_at": remote_asset["created_at"],
        "updated_at": remote_asset["updated_at"],
    }


def normalized_inventory_assets(value: object) -> list[dict]:
    require(isinstance(value, list), "remote release asset inventory assets must be a list")
    result = []
    seen_names: set[str] = set()
    seen_ids: set[int] = set()
    for row in value:
        require(isinstance(row, dict), "remote release asset inventory asset must be an object")
        require_exact_fields(row, ASSET_FIELDS, "remote release asset inventory asset")
        name = require_dist_name(row["name"], "remote release asset inventory asset name")
        require(name not in seen_names, f"remote release asset inventory duplicate asset name: {name}")
        seen_names.add(name)
        asset_id = require_positive_int(row["id"], f"remote release asset inventory asset {name} id")
        require(asset_id not in seen_ids, f"remote release asset inventory duplicate asset id: {asset_id}")
        seen_ids.add(asset_id)
        require(isinstance(row["kind"], str) and row["kind"], f"remote release asset inventory asset {name} kind invalid")
        require_non_negative_int(row["size_bytes"], f"remote release asset inventory asset {name} size_bytes")
        require_sha256(row["sha256"], f"remote release asset inventory asset {name} sha256")
        require_safe_string(row["download_url"], f"remote release asset inventory asset {name} download_url")
        for field in sorted(ASSET_TIMESTAMP_FIELDS):
            require_timestamp(row[field], f"remote release asset inventory asset {name} {field}")
        result.append(
            {
                "id": asset_id,
                "name": name,
                "kind": row["kind"],
                "size_bytes": row["size_bytes"],
                "sha256": row["sha256"],
                "download_url": row["download_url"],
                "created_at": row["created_at"],
                "updated_at": row["updated_at"],
            }
        )
    return sorted(result, key=lambda row: row["name"])


def asset_identity_rows(assets: list[dict]) -> list[dict]:
    return [
        {field: asset[field] for field in sorted(ASSET_IDENTITY_FIELDS)}
        for asset in sorted(assets, key=lambda row: row["name"])
    ]


def release_metadata_id(metadata: dict, release_tag: str) -> int:
    require(metadata.get("tag_name") == release_tag, "GitHub release metadata tag_name mismatch")
    return require_positive_int(metadata.get("id"), "GitHub release metadata id")


def require_download_url(value: object, release_tag: str, name: str) -> str:
    url = require_safe_string(value, f"GitHub release metadata asset {name} browser_download_url")
    expected_suffix = f"/releases/download/{release_tag}/{name}"
    require(
        url.startswith("https://") and url.endswith(expected_suffix),
        f"GitHub release metadata asset {name} browser_download_url must point at {expected_suffix}",
    )
    return url


def require_timestamp(value: object, label: str) -> str:
    require(parse_timestamp(value) is not None, f"{label} invalid")
    return value  # type: ignore[return-value]


def parse_timestamp(value: object) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def require_name_set(observed: set[str], expected: set[str], label: str) -> None:
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def comma_or_none(values: list[str]) -> str:
    return ", ".join(values) if values else "none"


def semicolon_or_none(values: list[str]) -> str:
    return "; ".join(values) if values else "none"


def recovery_remote_assets(rows: list[object]) -> tuple[dict[str, dict], list[str]]:
    by_name: dict[str, dict] = {}
    duplicate_details: list[str] = []
    seen_ids: dict[int, str] = {}
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            duplicate_details.append(f"assets[{index}]=not-an-object")
            continue
        name = row.get("name")
        if not isinstance(name, str) or not name:
            duplicate_details.append(f"assets[{index}]=missing-name")
            continue
        asset_id = row.get("id")
        if name in by_name:
            duplicate_details.append(f"name={name}")
        if isinstance(asset_id, int):
            if asset_id in seen_ids:
                duplicate_details.append(f"id={asset_id} names={seen_ids[asset_id]},{name}")
            seen_ids[asset_id] = name
        by_name[name] = row
    return by_name, duplicate_details


def recovery_mismatches(redownload_dir: Path, expected_by_name: dict[str, dict], remote_by_name: dict[str, dict]) -> list[str]:
    details: list[str] = []
    for name in sorted(set(expected_by_name) & set(remote_by_name)):
        expected = expected_by_name[name]
        remote = remote_by_name[name]
        remote_id = remote.get("id", "unknown")
        path = redownload_dir / name
        observed_sha = sha256_file(path) if path.is_file() else "missing-download"
        observed_size = path.stat().st_size if path.is_file() else "missing-download"
        if observed_sha != expected["sha256"]:
            details.append(f"{name}:id={remote_id}:expected_sha={expected['sha256']}:observed_sha={observed_sha}")
        remote_size = remote.get("size")
        if observed_size != expected["size_bytes"] or remote_size != expected["size_bytes"]:
            details.append(
                f"{name}:id={remote_id}:expected_size={expected['size_bytes']}:"
                f"downloaded_size={observed_size}:metadata_size={remote_size}"
            )
    return details


def recovery_asset_details(remote_by_name: dict[str, dict]) -> list[str]:
    details = []
    for name, row in sorted(remote_by_name.items()):
        details.append(f"{name}:id={row.get('id', 'unknown')}:size={row.get('size', 'unknown')}")
    return details


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


def require_sha256_ref(value: object, label: str) -> None:
    require(
        isinstance(value, str) and SHA256_REF_RE.fullmatch(value) is not None,
        f"{label} must be sha256:<lowercase digest>",
    )


def require_positive_int(value: object, label: str) -> int:
    require(isinstance(value, int) and value > 0, f"{label} must be a positive integer")
    return value


def require_non_negative_int(value: object, label: str) -> None:
    require(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")


def require_safe_string(value: object, label: str) -> str:
    require(
        isinstance(value, str)
        and value.strip()
        and all(ch >= " " and ch != "\x7f" for ch in value),
        f"{label} must be a nonempty printable string",
    )
    return value


def require_exact_fields(obj: dict, expected: set[str], label: str) -> None:
    observed = set(obj)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    require(
        not missing and not extra,
        f"{label} field mismatch: missing {comma_or_none(missing)}; extra {comma_or_none(extra)}",
    )


def read_json(path: Path, label: str) -> dict:
    payload = read_json_value(path, label)
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def read_json_value(path: Path, label: str) -> object:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        payload = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{label} is not valid JSON: {exc}") from exc
    return payload


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def file_ref(path: Path) -> dict:
    return {
        "name": path.name,
        "sha256": f"sha256:{sha256_file(path)}",
        "size_bytes": path.stat().st_size,
    }


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
