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
PUBLIC_ASSET_FIELDS = {"name", "kind", "sha256", "size_bytes", "integrity_subject"}

SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
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
    inventory_path = (args.inventory or redownload_dir / INVENTORY_NAME).resolve()

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
    print(f"remote release asset inventory ok: {inventory_path}")
    return 0


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
    require(observed_assets == expected_assets, "remote release asset inventory assets mismatch")


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
        require_timestamp(row["created_at"], f"remote release asset inventory asset {name} created_at")
        require_timestamp(row["updated_at"], f"remote release asset inventory asset {name} updated_at")
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
