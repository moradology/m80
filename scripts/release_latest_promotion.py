#!/usr/bin/env python3
"""Approve or refuse moving the public latest pointer for a release tag."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import sys
from typing import Any


SCHEMA_VERSION = 1
KIND = "m80_release_latest_promotion_decision"
ROLLBACK_KIND = "m80_release_latest_rollback_receipt"
STABLE_TAG_RE = re.compile(r"^v([0-9]+)[.]([0-9]+)[.]([0-9]+)$")
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
RAW_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
DIST_NAME_RE = re.compile(r"^[A-Za-z0-9._+-]+$")
REMOTE_INVENTORY_KIND = "m80_release_remote_asset_inventory"
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
REMOTE_INVENTORY_NAME = "m80-release-remote-assets.json"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--release-list", required=True, type=Path)
    parser.add_argument("--publish-decision", required=True, type=Path)
    parser.add_argument("--proof-ledger", required=True, type=Path)
    parser.add_argument("--upload-manifest", required=True, type=Path)
    parser.add_argument("--remote-inventory", required=True, type=Path)
    parser.add_argument("--rollback-receipt", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--generated-at")
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        decision = build_decision(
            release_tag=args.release_tag,
            release_list=read_json(args.release_list, "release list"),
            publish_decision=args.publish_decision,
            proof_ledger=args.proof_ledger,
            upload_manifest=args.upload_manifest,
            remote_inventory=args.remote_inventory,
            rollback_receipt=args.rollback_receipt,
            generated_at=args.generated_at,
        )
        verify_decision(decision)
    except PromotionError as exc:
        decision = exc.decision
        verify_decision(decision)
        if args.write:
            write_json(args.out, decision)
        print(exc, file=sys.stderr)
        return 1

    if args.write:
        write_json(args.out, decision)
    else:
        current = read_json(args.out, "latest promotion decision")
        if current != decision:
            print(f"{args.out}: latest promotion decision is stale", file=sys.stderr)
            return 1
    print(f"latest promotion approved: {args.release_tag}")
    return 0


def build_decision(
    *,
    release_tag: str,
    release_list: Any,
    publish_decision: Path,
    proof_ledger: Path,
    upload_manifest: Path,
    remote_inventory: Path,
    rollback_receipt: Path | None,
    generated_at: str | None,
) -> dict[str, Any]:
    target_semver = require_stable_tag(release_tag)
    stable_tags = stable_public_tags(release_list)
    highest_tag = max(stable_tags, key=stable_tags.get) if stable_tags else release_tag
    highest_semver = stable_tags.get(highest_tag, target_semver)
    publish_digest = file_digest(publish_decision)
    ledger_digest = file_digest(proof_ledger)
    inventory = read_json(remote_inventory, "remote release asset inventory")
    manifest = read_json(upload_manifest, "release upload manifest")
    inventory_digest = file_digest(remote_inventory)
    inventory_errors = validate_remote_inventory(
        inventory,
        upload_manifest=manifest,
        release_tag=release_tag,
    )
    if inventory_errors:
        decision = decision_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            decision="refused",
            highest_stable_public_tag=highest_tag,
            highest_stable_public_semver=list(highest_semver),
            rollback_receipt_digest=None,
            publish_decision_digest=publish_digest,
            proof_ledger_digest=ledger_digest,
            remote_inventory_digest=inventory_digest,
            reason="remote inventory is invalid: " + "; ".join(inventory_errors),
            remediation="redownload public release assets and regenerate m80-release-remote-assets.json before moving latest",
        )
        raise PromotionError("latest promotion refused: remote inventory is invalid", decision)

    if target_semver >= highest_semver:
        return decision_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            decision="approved",
            highest_stable_public_tag=highest_tag,
            highest_stable_public_semver=list(highest_semver),
            rollback_receipt_digest=None,
            publish_decision_digest=publish_digest,
            proof_ledger_digest=ledger_digest,
            remote_inventory_digest=inventory_digest,
            reason="target release is not older than the highest public stable release",
            remediation=None,
        )

    if rollback_receipt is None:
        decision = decision_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            decision="refused",
            highest_stable_public_tag=highest_tag,
            highest_stable_public_semver=list(highest_semver),
            rollback_receipt_digest=None,
            publish_decision_digest=publish_digest,
            proof_ledger_digest=ledger_digest,
            remote_inventory_digest=inventory_digest,
            reason="target release is older than the highest public stable release",
            remediation=(
                "publish a newer stable tag or commit docs/operations/"
                "release-latest-rollback-receipt.json that matches this release, "
                "publish decision, and proof ledger"
            ),
        )
        raise PromotionError("latest promotion refused: target is older than highest public stable release", decision)

    receipt = read_json(rollback_receipt, "latest rollback receipt")
    receipt_digest = file_digest(rollback_receipt)
    receipt_errors = validate_rollback_receipt(
        receipt,
        release_tag=release_tag,
        highest_tag=highest_tag,
        publish_decision_digest=publish_digest,
        proof_ledger_digest=ledger_digest,
    )
    if receipt_errors:
        decision = decision_payload(
            release_tag=release_tag,
            generated_at=generated_at,
            decision="refused",
            highest_stable_public_tag=highest_tag,
            highest_stable_public_semver=list(highest_semver),
            rollback_receipt_digest=receipt_digest,
            publish_decision_digest=publish_digest,
            proof_ledger_digest=ledger_digest,
            remote_inventory_digest=inventory_digest,
            reason="rollback receipt is invalid: " + "; ".join(receipt_errors),
            remediation="fix or remove docs/operations/release-latest-rollback-receipt.json",
        )
        raise PromotionError("latest promotion refused: rollback receipt is invalid", decision)

    return decision_payload(
        release_tag=release_tag,
        generated_at=generated_at,
        decision="rollback_approved",
        highest_stable_public_tag=highest_tag,
        highest_stable_public_semver=list(highest_semver),
        rollback_receipt_digest=receipt_digest,
        publish_decision_digest=publish_digest,
        proof_ledger_digest=ledger_digest,
        remote_inventory_digest=inventory_digest,
        reason=require_nonempty_str(receipt, "reason", "rollback receipt"),
        remediation=None,
    )


def validate_rollback_receipt(
    receipt: Any,
    *,
    release_tag: str,
    highest_tag: str,
    publish_decision_digest: str,
    proof_ledger_digest: str,
) -> list[str]:
    errors: list[str] = []
    if not isinstance(receipt, dict):
        return ["receipt must be a JSON object"]
    expected_fields = {
        "schema_version",
        "kind",
        "decision",
        "release_tag",
        "highest_stable_public_tag",
        "reason",
        "actor",
        "publish_decision_digest",
        "proof_ledger_digest",
        "generated_at",
    }
    extra = sorted(set(receipt) - expected_fields)
    missing = sorted(expected_fields - set(receipt))
    if extra:
        errors.append("unknown fields: " + ", ".join(extra))
    if missing:
        errors.append("missing fields: " + ", ".join(missing))
    if receipt.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}")
    if receipt.get("kind") != ROLLBACK_KIND:
        errors.append(f"kind must be {ROLLBACK_KIND}")
    if receipt.get("decision") != "approved":
        errors.append("decision must be approved")
    if receipt.get("release_tag") != release_tag:
        errors.append(f"release_tag must be {release_tag}")
    if receipt.get("highest_stable_public_tag") != highest_tag:
        errors.append(f"highest_stable_public_tag must be {highest_tag}")
    for field in ["reason", "actor", "generated_at"]:
        value = receipt.get(field)
        if not isinstance(value, str) or not value.strip():
            errors.append(f"{field} must be a nonempty string")
    if receipt.get("publish_decision_digest") != publish_decision_digest:
        errors.append("publish_decision_digest mismatch")
    if receipt.get("proof_ledger_digest") != proof_ledger_digest:
        errors.append("proof_ledger_digest mismatch")
    for field in ["publish_decision_digest", "proof_ledger_digest"]:
        value = receipt.get(field)
        if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
            errors.append(f"{field} must be sha256:<64 lowercase hex>")
    return errors


def validate_remote_inventory(inventory: Any, *, upload_manifest: Any, release_tag: str) -> list[str]:
    errors: list[str] = []
    manifest_assets, manifest_errors = normalized_manifest_assets(upload_manifest, release_tag)
    errors.extend(manifest_errors)
    inventory_assets, inventory_errors = normalized_inventory_assets(inventory, release_tag)
    errors.extend(inventory_errors)
    if errors:
        return errors

    manifest_by_name = {asset["name"]: asset for asset in manifest_assets}
    inventory_by_name = {asset["name"]: asset for asset in inventory_assets}
    missing = sorted(set(manifest_by_name) - set(inventory_by_name))
    extra = sorted(set(inventory_by_name) - set(manifest_by_name))
    if missing:
        errors.append("remote inventory missing asset(s): " + ", ".join(missing))
    if extra:
        errors.append("remote inventory extra asset(s): " + ", ".join(extra))
    for name in sorted(set(manifest_by_name) & set(inventory_by_name)):
        manifest_asset = manifest_by_name[name]
        inventory_asset = inventory_by_name[name]
        for field in ("kind", "sha256", "size_bytes"):
            if manifest_asset[field] != inventory_asset[field]:
                errors.append(f"remote inventory {name} {field} mismatch")
    return errors


def normalized_manifest_assets(manifest: Any, release_tag: str) -> tuple[list[dict[str, Any]], list[str]]:
    errors: list[str] = []
    if not isinstance(manifest, dict):
        return [], ["release upload manifest must be a JSON object"]
    if manifest.get("release_tag") != release_tag:
        errors.append(f"release upload manifest release_tag must be {release_tag}")
    assets = manifest.get("public_assets")
    if not isinstance(assets, list):
        return [], errors + ["release upload manifest public_assets must be a list"]
    result: list[dict[str, Any]] = []
    seen: set[str] = set()
    for index, row in enumerate(assets):
        label = f"release upload manifest public_assets[{index}]"
        if not isinstance(row, dict):
            errors.append(f"{label} must be an object")
            continue
        asset = normalized_asset_row(row, label, require_id=False, errors=errors)
        if asset is None:
            continue
        if asset["name"] in seen:
            errors.append(f"release upload manifest duplicate asset name: {asset['name']}")
        seen.add(asset["name"])
        result.append(asset)
    return result, errors


def normalized_inventory_assets(inventory: Any, release_tag: str) -> tuple[list[dict[str, Any]], list[str]]:
    errors: list[str] = []
    if not isinstance(inventory, dict):
        return [], ["remote inventory must be a JSON object"]
    if inventory.get("kind") != REMOTE_INVENTORY_KIND:
        errors.append(f"remote inventory kind must be {REMOTE_INVENTORY_KIND}")
    if inventory.get("release_tag") != release_tag:
        errors.append(f"remote inventory release_tag must be {release_tag}")
    assets = inventory.get("assets")
    if not isinstance(assets, list):
        return [], errors + ["remote inventory assets must be a list"]
    result: list[dict[str, Any]] = []
    seen_names: set[str] = set()
    seen_ids: set[int] = set()
    for index, row in enumerate(assets):
        label = f"remote inventory assets[{index}]"
        if not isinstance(row, dict):
            errors.append(f"{label} must be an object")
            continue
        asset = normalized_asset_row(row, label, require_id=True, errors=errors)
        if asset is None:
            continue
        if asset["name"] in seen_names:
            errors.append(f"remote inventory duplicate asset name: {asset['name']}")
        seen_names.add(asset["name"])
        asset_id = asset["id"]
        if asset_id in seen_ids:
            errors.append(f"remote inventory duplicate asset id: {asset_id}")
        seen_ids.add(asset_id)
        for field in ["created_at", "updated_at"]:
            value = row.get(field)
            if parse_timestamp(value) is None:
                errors.append(f"{label} {field} must be an ISO-8601 timestamp")
        result.append(asset)
    return result, errors


def normalized_asset_row(
    row: dict[str, Any],
    label: str,
    *,
    require_id: bool,
    errors: list[str],
) -> dict[str, Any] | None:
    name = row.get("name")
    kind = row.get("kind")
    sha256 = row.get("sha256")
    size_bytes = row.get("size_bytes")
    ok = True
    if not isinstance(name, str) or DIST_NAME_RE.fullmatch(name) is None or name in {"", ".", ".."}:
        errors.append(f"{label} name must be a flat asset name")
        ok = False
    if not isinstance(kind, str) or not kind.strip():
        errors.append(f"{label} kind must be a nonempty string")
        ok = False
    if not isinstance(sha256, str) or RAW_SHA256_RE.fullmatch(sha256) is None:
        errors.append(f"{label} sha256 must be a lowercase sha256")
        ok = False
    if not isinstance(size_bytes, int) or size_bytes < 0:
        errors.append(f"{label} size_bytes must be a non-negative integer")
        ok = False
    asset_id = None
    if require_id:
        asset_id = row.get("id")
        if not isinstance(asset_id, int) or asset_id <= 0:
            errors.append(f"{label} id must be a positive integer")
            ok = False
    if not ok:
        return None
    result = {"name": name, "kind": kind, "sha256": sha256, "size_bytes": size_bytes}
    if require_id:
        result["id"] = asset_id
    return result


def stable_public_tags(release_list: Any) -> dict[str, tuple[int, int, int]]:
    releases = normalize_release_list(release_list)
    result: dict[str, tuple[int, int, int]] = {}
    for release in releases:
        tag = first_str(release, "tagName", "tag_name", "name")
        if tag is None:
            continue
        semver = parse_stable_tag(tag)
        if semver is None:
            continue
        if first_bool(release, "isDraft", "draft") is True:
            continue
        if first_bool(release, "isPrerelease", "prerelease") is True:
            continue
        result[tag] = semver
    return result


def normalize_release_list(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, list):
        return [item for item in value if isinstance(item, dict)]
    if isinstance(value, dict):
        for key in ["releases", "items"]:
            items = value.get(key)
            if isinstance(items, list):
                return [item for item in items if isinstance(item, dict)]
    raise PromotionError(
        "latest promotion refused: release list must be an array or object with releases",
        decision_payload(
            release_tag="unknown",
            generated_at=None,
            decision="refused",
            highest_stable_public_tag=None,
            highest_stable_public_semver=None,
            rollback_receipt_digest=None,
            publish_decision_digest="sha256:" + "0" * 64,
            proof_ledger_digest="sha256:" + "0" * 64,
            remote_inventory_digest="sha256:" + "0" * 64,
            reason="release list shape is invalid",
            remediation="capture GitHub release list JSON before latest promotion",
        ),
    )


def decision_payload(
    *,
    release_tag: str,
    generated_at: str | None,
    decision: str,
    highest_stable_public_tag: str | None,
    highest_stable_public_semver: list[int] | None,
    rollback_receipt_digest: str | None,
    publish_decision_digest: str,
    proof_ledger_digest: str,
    remote_inventory_digest: str,
    reason: str,
    remediation: str | None,
) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "release_tag": release_tag,
        "generated_at": generated_at or utc_now(),
        "decision": decision,
        "highest_stable_public_tag": highest_stable_public_tag,
        "highest_stable_public_semver": highest_stable_public_semver,
        "rollback_receipt_digest": rollback_receipt_digest,
        "publish_decision_digest": publish_decision_digest,
        "proof_ledger_digest": proof_ledger_digest,
        "remote_inventory_digest": remote_inventory_digest,
        "reason": reason,
        "remediation": remediation,
    }


def verify_decision(decision: dict[str, Any]) -> None:
    required = {
        "schema_version",
        "kind",
        "release_tag",
        "generated_at",
        "decision",
        "highest_stable_public_tag",
        "highest_stable_public_semver",
        "rollback_receipt_digest",
        "publish_decision_digest",
        "proof_ledger_digest",
        "remote_inventory_digest",
        "reason",
        "remediation",
    }
    require(set(decision) == required, "latest promotion decision fields mismatch")
    require(decision["schema_version"] == SCHEMA_VERSION, "latest promotion decision schema_version mismatch")
    require(decision["kind"] == KIND, "latest promotion decision kind mismatch")
    require(decision["decision"] in {"approved", "rollback_approved", "refused"}, "latest promotion decision invalid")
    require(parse_stable_tag(decision["release_tag"]) is not None, "latest promotion decision release_tag invalid")
    require(isinstance(decision["generated_at"], str) and decision["generated_at"], "generated_at invalid")
    require(
        decision["highest_stable_public_tag"] is None
        or parse_stable_tag(decision["highest_stable_public_tag"]) is not None,
        "highest_stable_public_tag invalid",
    )
    semver = decision["highest_stable_public_semver"]
    require(
        semver is None
        or (
            isinstance(semver, list)
            and len(semver) == 3
            and all(isinstance(part, int) and part >= 0 for part in semver)
        ),
        "highest_stable_public_semver invalid",
    )
    for field in ["publish_decision_digest", "proof_ledger_digest", "remote_inventory_digest"]:
        require(isinstance(decision[field], str) and SHA256_RE.fullmatch(decision[field]) is not None, f"{field} invalid")
    rollback_digest = decision["rollback_receipt_digest"]
    require(
        rollback_digest is None
        or (isinstance(rollback_digest, str) and SHA256_RE.fullmatch(rollback_digest) is not None),
        "rollback_receipt_digest invalid",
    )
    require(isinstance(decision["reason"], str) and decision["reason"], "reason invalid")
    require(
        decision["remediation"] is None
        or (isinstance(decision["remediation"], str) and decision["remediation"]),
        "remediation invalid",
    )


def parse_stable_tag(tag: str) -> tuple[int, int, int] | None:
    match = STABLE_TAG_RE.fullmatch(tag)
    if match is None:
        return None
    return tuple(int(part) for part in match.groups())


def require_stable_tag(tag: str) -> tuple[int, int, int]:
    semver = parse_stable_tag(tag)
    require(semver is not None, f"release tag must be stable vMAJOR.MINOR.PATCH: {tag}")
    return semver


def first_str(obj: dict[str, Any], *keys: str) -> str | None:
    for key in keys:
        value = obj.get(key)
        if isinstance(value, str) and value:
            return value
    return None


def first_bool(obj: dict[str, Any], *keys: str) -> bool | None:
    for key in keys:
        value = obj.get(key)
        if isinstance(value, bool):
            return value
    return None


def parse_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def file_digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def require_nonempty_str(obj: dict[str, Any], key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value.strip(), f"{label} {key} must be a nonempty string")
    return value


def read_json(path: Path, label: str) -> Any:
    try:
        return json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise SystemExit(f"{label} missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid {label} JSON: {exc}") from exc


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


class PromotionError(Exception):
    def __init__(self, message: str, decision: dict[str, Any]) -> None:
        super().__init__(message)
        self.decision = decision


if __name__ == "__main__":
    raise SystemExit(main())
