#!/usr/bin/env python3
"""Structured proof schema for the public latest freshness verifier."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re

from quickstart_snippets import PublicCommandSnippet
from release_url_contract import public_release_root


PROOF_SCHEMA_VERSION = 1
PROOF_STATUSES = frozenset({"success", "failure", "not_started"})
SECTION_STATUSES = frozenset({"success", "failure", "not_run", "unavailable"})
SHA256_URI_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
RFC3339_UTC_RE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
REQUIRED_TOP_LEVEL_FIELDS = (
    "schema_version",
    "status",
    "generated_at",
    "repository",
    "workflow_run_id",
    "resolved_latest_tag",
    "public_command_inventory",
    "tag_agreement",
    "integrity_result",
    "fixture_install_result",
    "failure_taxonomy",
    "substrate",
    "fetch_policy",
    "checked_urls",
    "public_assets",
    "safety_floor",
)


def command_inventory_proof(snippets: list[PublicCommandSnippet]) -> dict:
    entries = sorted(
        (
            {
                "path": str(snippet.path),
                "line": snippet.line,
                "classification": snippet.classification,
                "body_sha256": sha256_uri(snippet.body.encode("utf-8")),
            }
            for snippet in snippets
        ),
        key=command_inventory_entry_key,
    )
    return {
        "status": "success",
        "digest": command_inventory_digest(entries),
        "count": len(entries),
        "entries": entries,
    }


def unavailable_command_inventory(message: str) -> dict:
    return {
        "status": "unavailable",
        "digest": None,
        "count": 0,
        "entries": [],
        "error": safe_excerpt(message),
    }


def success_proof(
    *,
    repository: str,
    resolved_tag: str,
    generated_at: str,
    fetch_policy: dict,
    checked_urls: list[dict],
    public_assets: list[dict],
    public_command_inventory: dict,
    failure_classes: list[str],
    latest_source_mode: str,
    guard_source_mode: str,
    tag_agreement: dict | None = None,
    fixture_install_result: dict | None = None,
) -> dict:
    root = public_release_root()
    if tag_agreement is None:
        tag_agreement = {
            "status": "success",
            "latest_tag": resolved_tag,
            "guard_tag": resolved_tag,
            "latest_install_url": root.latest_install_url,
            "pinned_install_url": root.pinned_install_url(resolved_tag),
            "latest_source_mode": latest_source_mode,
            "guard_source_mode": guard_source_mode,
        }
    return base_proof(
        status="success",
        generated_at=generated_at,
        repository=repository,
        resolved_latest_tag=resolved_tag,
        public_command_inventory=public_command_inventory,
        fetch_policy=fetch_policy,
        checked_urls=checked_urls,
        public_assets=public_assets,
        latest_source_mode=latest_source_mode,
        guard_source_mode=guard_source_mode,
    ) | {
        "resolved_tag": resolved_tag,
        "published_at": generated_at,
        "freshness_network_bounded": True,
        "tag_agreement": tag_agreement,
        "integrity_result": {
            "status": "success",
            "public_asset_count": len(public_assets),
            "assets": [
                {
                    "name": asset["name"],
                    "sha256": asset["sha256"],
                    "checksum_sources": asset["checksum_sources"],
                }
                for asset in public_assets
            ],
        },
        "fixture_install_result": fixture_install_result
        or {
            "status": "not_run",
            "reason": "hostless install-root fixture was not requested",
        },
        "failure_taxonomy": {
            "status": "success",
            "known_classes": failure_classes,
        },
        "safety_floor": empty_safety_floor(generated_at),
    }


def failure_proof(
    *,
    repository: str,
    generated_at: str,
    failure_class: str,
    failure_message: str,
    repair_command: str | None,
    fetch_policy: dict,
    checked_urls: list[dict],
    public_assets: list[dict],
    public_command_inventory: dict,
    failure_classes: list[str],
    latest_source_mode: str,
    guard_source_mode: str,
    resolved_latest_tag: str | None = None,
) -> dict:
    failure = {
        "class": failure_class or "verifier-schema-drift",
        "message": safe_excerpt(failure_message),
        "repair_command": repair_command,
    }
    return base_proof(
        status="failure",
        generated_at=generated_at,
        repository=repository,
        resolved_latest_tag=resolved_latest_tag,
        public_command_inventory=public_command_inventory,
        fetch_policy=fetch_policy,
        checked_urls=checked_urls,
        public_assets=public_assets,
        latest_source_mode=latest_source_mode,
        guard_source_mode=guard_source_mode,
    ) | {
        "resolved_tag": resolved_latest_tag,
        "published_at": generated_at,
        "freshness_network_bounded": True,
        "tag_agreement": {
            "status": "failure" if failure["class"] == "stale-latest" else "not_run",
            "latest_tag": resolved_latest_tag,
            "guard_tag": None,
            "latest_source_mode": latest_source_mode,
            "guard_source_mode": guard_source_mode,
        },
        "integrity_result": {
            "status": "failure"
            if failure["class"] in {"checksum-mismatch", "missing-public-asset", "provenance-mismatch"}
            else "not_run",
            "public_asset_count": len(public_assets),
            "assets": [
                {
                    "name": asset["name"],
                    "sha256": asset["sha256"],
                    "checksum_sources": asset.get("checksum_sources", []),
                }
                for asset in public_assets
            ],
        },
        "fixture_install_result": {
            "status": "not_run",
            "reason": "freshness verifier failed before hostless fixture install",
        },
        "failure_taxonomy": {
            "status": "failure",
            "known_classes": failure_classes,
            "failure": failure,
        },
        "failure": failure,
        "safety_floor": empty_safety_floor(generated_at),
    }


def base_proof(
    *,
    status: str,
    generated_at: str,
    repository: str,
    resolved_latest_tag: str | None,
    public_command_inventory: dict,
    fetch_policy: dict,
    checked_urls: list[dict],
    public_assets: list[dict],
    latest_source_mode: str,
    guard_source_mode: str,
) -> dict:
    return {
        "schema_version": PROOF_SCHEMA_VERSION,
        "status": status,
        "generated_at": generated_at,
        "repository": repository,
        "workflow_run_id": os.environ.get("GITHUB_RUN_ID") or None,
        "workflow": {
            "name": os.environ.get("GITHUB_WORKFLOW") or None,
            "run_id": os.environ.get("GITHUB_RUN_ID") or None,
            "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT") or None,
            "job": os.environ.get("GITHUB_JOB") or None,
        },
        "resolved_latest_tag": resolved_latest_tag,
        "public_command_inventory": public_command_inventory,
        "substrate": substrate_summary(latest_source_mode, guard_source_mode),
        "fetch_policy": fetch_policy,
        "checked_urls": checked_urls,
        "public_assets": public_assets,
    }


def substrate_summary(latest_source_mode: str, guard_source_mode: str) -> dict:
    root = public_release_root()
    return {
        "network_target": "public-github-release",
        "auth_state": "unauthenticated-public-read",
        "public_owner": root.owner,
        "public_repo": root.repo,
        "latest_source_mode": latest_source_mode,
        "guard_source_mode": guard_source_mode,
        "fixture_source": latest_source_mode != "url" or guard_source_mode != "url",
        "github_write_apis_available": False,
    }


def empty_safety_floor(generated_at: str) -> dict:
    return {
        "schema_version": 1,
        "published_at": generated_at,
        "minimum_safe_tag": None,
        "yanked_releases": [],
    }


def validate_freshness_proof(
    proof: dict,
    *,
    expected_command_inventory_digest: str | None = None,
) -> list[str]:
    errors: list[str] = []
    if not isinstance(proof, dict):
        return ["proof must be a JSON object"]

    for field in REQUIRED_TOP_LEVEL_FIELDS:
        if field not in proof:
            errors.append(f"missing required field: {field}")
    if errors:
        return errors

    if proof["schema_version"] != PROOF_SCHEMA_VERSION:
        errors.append(f"schema_version must be {PROOF_SCHEMA_VERSION}")
    if proof["status"] not in PROOF_STATUSES:
        errors.append(f"unknown proof status: {proof['status']!r}")
    if not isinstance(proof["generated_at"], str) or RFC3339_UTC_RE.fullmatch(proof["generated_at"]) is None:
        errors.append("generated_at must be UTC RFC3339 seconds")

    validate_section_status(proof, "tag_agreement", errors)
    validate_section_status(proof, "integrity_result", errors)
    validate_section_status(proof, "fixture_install_result", errors)
    validate_section_status(proof, "failure_taxonomy", errors)
    validate_command_inventory(
        proof["public_command_inventory"],
        errors,
        expected_command_inventory_digest=expected_command_inventory_digest,
    )
    validate_checked_urls(proof["checked_urls"], errors)
    validate_public_assets(proof["public_assets"], errors)
    return errors


def fetch_policy(
    *,
    connect_timeout_seconds: int,
    max_time_seconds: int,
    retry_count: int,
    retry_delay_seconds: int,
) -> dict:
    return {
        "connect_timeout_seconds": connect_timeout_seconds,
        "max_time_seconds": max_time_seconds,
        "retry_count": retry_count,
        "retry_delay_seconds": retry_delay_seconds,
    }


def checked_url_proof_rows(checks: list) -> list[dict]:
    return [
        {
            "role": check.role,
            "url": check.url,
            "release_tag": check.release_tag,
            "asset_name": check.asset_name,
            "sources": sorted(check.sources),
            "size_bytes": check.size_bytes,
            "sha256": check.sha256,
        }
        for check in sorted(checks, key=check_object_key)
    ]


def public_asset_proof_rows(public_assets: dict, checksum_sources: dict[str, list[str]]) -> list[dict]:
    return [
        {
            "name": asset.name,
            "role": asset.role,
            "url": asset.url,
            "release_tag": asset.release_tag,
            "size_bytes": asset.size_bytes,
            "sha256": asset.sha256,
            "checksum_sources": checksum_sources.get(asset.name, []),
        }
        for _name, asset in sorted(public_assets.items())
    ]


def validate_section_status(proof: dict, field: str, errors: list[str]) -> None:
    value = proof.get(field)
    if not isinstance(value, dict):
        errors.append(f"{field} must be an object")
        return
    status = value.get("status")
    if status not in SECTION_STATUSES:
        errors.append(f"{field} has unknown status: {status!r}")


def validate_command_inventory(
    inventory: object,
    errors: list[str],
    *,
    expected_command_inventory_digest: str | None,
) -> None:
    if not isinstance(inventory, dict):
        errors.append("public_command_inventory must be an object")
        return
    status = inventory.get("status")
    if status not in SECTION_STATUSES:
        errors.append(f"public_command_inventory has unknown status: {status!r}")
        return
    entries = inventory.get("entries")
    if not isinstance(entries, list):
        errors.append("public_command_inventory.entries must be a list")
        return
    if entries != sorted(entries, key=command_inventory_entry_key):
        errors.append("public_command_inventory.entries must be sorted deterministically")
    if inventory.get("count") != len(entries):
        errors.append("public_command_inventory.count does not match entries")
    digest = inventory.get("digest")
    if status == "success":
        if not isinstance(digest, str) or SHA256_URI_RE.fullmatch(digest) is None:
            errors.append("public_command_inventory.digest must be sha256:<64 hex>")
        elif digest != command_inventory_digest(entries):
            errors.append("public_command_inventory.digest does not match entries")
    if expected_command_inventory_digest is not None and digest != expected_command_inventory_digest:
        errors.append("checked_command_inventory_digest does not match proof")


def validate_checked_urls(value: object, errors: list[str]) -> None:
    if not isinstance(value, list):
        errors.append("checked_urls must be a list")
        return
    if value != sorted(value, key=checked_url_key):
        errors.append("checked_urls must be sorted deterministically")
    for row in value:
        if not isinstance(row, dict):
            errors.append("checked_urls entries must be objects")
            return
        sources = row.get("sources")
        if not isinstance(sources, list) or sources != sorted(sources):
            errors.append(f"checked_urls sources must be sorted for {row.get('url')!r}")


def validate_public_assets(value: object, errors: list[str]) -> None:
    if not isinstance(value, list):
        errors.append("public_assets must be a list")
        return
    if value != sorted(value, key=lambda row: row.get("name") if isinstance(row, dict) else ""):
        errors.append("public_assets must be sorted deterministically")


def command_inventory_digest(entries: list[dict]) -> str:
    return sha256_uri(canonical_json(entries).encode("utf-8"))


def command_inventory_entry_key(entry: dict) -> tuple:
    return (
        entry.get("path", ""),
        entry.get("line", 0),
        entry.get("classification", ""),
        entry.get("body_sha256", ""),
    )


def checked_url_key(row: dict) -> tuple:
    return (
        row.get("role", ""),
        row.get("release_tag", ""),
        row.get("asset_name", ""),
        row.get("url", ""),
        tuple(row.get("sources", [])),
    )


def check_object_key(check) -> tuple:
    return (check.role, check.release_tag, check.asset_name, check.url, check.sources)


def canonical_json(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def sha256_uri(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def safe_excerpt(text: str, limit: int = 4000) -> str:
    return text[-limit:]
