#!/usr/bin/env python3
"""Bounded public release freshness checks for the m80 install surface."""

from __future__ import annotations

import argparse
import base64
import binascii
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile

from freshness_proof import checked_url_proof_rows, command_inventory_proof, fetch_policy, failure_proof, public_asset_proof_rows, success_proof, unavailable_command_inventory, validate_freshness_proof
from quickstart_snippets import INSTALL_URL_RE, public_command_inventory
from release_url_contract import pinned_install_command, public_release_root, release_asset_url
from stable_latest_bootstrap import (
    METADATA_CONNECT_TIMEOUT_SECONDS,
    METADATA_MAX_TIME_SECONDS,
    METADATA_RETRY_COUNT,
    METADATA_RETRY_DELAY_SECONDS,
    load_latest_source,
    resolve_latest_bootstrap,
)
from stable_release_channel import read_json
from stable_release_channel import (
    BUNDLE_NAME,
    INTEGRITY_ATTESTATION_BUNDLE_NAME,
    METADATA_NAME,
    REQUIRED_PUBLIC_ASSETS,
    SCHEMA_VERSION,
    public_asset_role,
)


FETCH_CONNECT_TIMEOUT_SECONDS = METADATA_CONNECT_TIMEOUT_SECONDS
FETCH_MAX_TIME_SECONDS = METADATA_MAX_TIME_SECONDS
FETCH_RETRY_COUNT = METADATA_RETRY_COUNT
FETCH_RETRY_DELAY_SECONDS = METADATA_RETRY_DELAY_SECONDS
SHA256_DIGEST_RE = re.compile(r"^sha256:([0-9a-f]{64})$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
FRESHNESS_FAILURE_CLASSES = frozenset(
    {
        "network-transient",
        "stale-latest",
        "missing-public-asset",
        "docs-drift",
        "checksum-mismatch",
        "provenance-mismatch",
        "public-release-unavailable",
        "real-kvm-substrate-unavailable",
        "verifier-schema-drift",
    }
)
NETWORK_FAILURE_KINDS = frozenset(
    {
        "curl_spawn_failed",
        "dns_or_connect_failure",
        "http_failure",
        "metadata_fetch_failure",
        "partial_download",
        "public_url_fetch_failure",
        "redirect_loop",
        "timeout",
    }
)


@dataclass(frozen=True)
class FreshnessAsset:
    name: str
    role: str
    url: str
    release_tag: str
    size_bytes: int | None
    sha256: str


@dataclass(frozen=True)
class FreshnessUrl:
    role: str
    url: str
    asset_name: str
    release_tag: str
    sources: tuple[str, ...]
    size_bytes: int | None = None
    sha256: str | None = None


def parse_args() -> argparse.Namespace:
    root = public_release_root()
    parser = argparse.ArgumentParser(description=__doc__)
    latest = parser.add_mutually_exclusive_group()
    latest.add_argument(
        "--latest-url",
        default=f"https://api.github.com/repos/{root.repository}/releases/latest",
        help="GitHub latest release metadata URL",
    )
    latest.add_argument("--latest-metadata", type=Path, help="local latest release metadata fixture")
    guard = parser.add_mutually_exclusive_group()
    guard.add_argument("--guard-latest-url", help="second latest metadata URL used for tag-switch guard")
    guard.add_argument("--guard-metadata", type=Path, help="second local latest metadata fixture used for tag-switch guard")
    parser.add_argument("--asset-index", type=Path, help="optional local release asset index fixture")
    parser.add_argument("--docs-root", type=Path, default=Path("."), help="repository root for docs URL inventory")
    parser.add_argument("--curl", default="curl", help="curl binary used for URL fetches")
    parser.add_argument("--json", action="store_true", help="render the freshness proof summary as JSON")
    parser.add_argument("--proof-out", type=Path, help="write the freshness proof summary JSON to this path")
    parser.add_argument("--validate-proof", type=Path, help="validate an existing freshness proof and exit")
    parser.add_argument(
        "--hostless-install-fixture",
        action="store_true",
        help="run the public install.sh path into a fixture install root without host mutation",
    )
    parser.add_argument(
        "--hostless-fixture-root",
        type=Path,
        help="fixture work root for --hostless-install-fixture; explicit roots are preserved for inspection",
    )
    parser.add_argument(
        "--hostless-install-sudo",
        action="store_true",
        help="run the fixture installer through sudo -n env; records that live host privilege was used",
    )
    parser.add_argument(
        "--expected-command-inventory-digest",
        help="expected public command inventory digest for --validate-proof",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.validate_proof is not None:
        proof = read_json(args.validate_proof, "freshness proof")
        errors = validate_freshness_proof(
            proof,
            expected_command_inventory_digest=args.expected_command_inventory_digest,
        )
        if errors:
            raise SystemExit("freshness proof invalid: " + "; ".join(errors))
        print(f"freshness proof valid: path={args.validate_proof} status={proof['status']}")
        return 0

    asset_index = read_json(args.asset_index, "release asset index") if args.asset_index else None
    generated_at = utc_now_rfc3339()
    release_root = public_release_root()
    repository = release_root.repository
    resolved_tag: str | None = None
    latest_source_mode = "unknown"
    guard_source_mode = "unknown"
    command_inventory = None
    command_inventory_summary = None
    checks: list[FreshnessUrl] = []
    public_assets: dict[str, FreshnessAsset] = {}
    checksum_sources: dict[str, list[str]] = {}
    provenance_result: dict | None = None
    try:
        command_inventory = public_command_inventory(args.docs_root)
        command_inventory_summary = command_inventory_proof(command_inventory)
        latest = load_latest_source(
            args.latest_metadata,
            args.latest_url,
            curl_bin=args.curl,
            label="latest release metadata",
        )
        latest_source_mode = latest.mode
        public_assets = public_release_assets(latest.release, asset_index=asset_index)
        guard = load_freshness_guard_source(args, latest)
        guard_source_mode = guard.mode
        resolution = resolve_latest_bootstrap(latest, guard=guard, asset_index=asset_index)
        repository = resolution.repository
        resolved_tag = resolution.resolved_tag
        checks = freshness_urls(
            resolution.resolved_tag,
            resolution.pinned_asset_urls,
            command_inventory=command_inventory,
            public_assets=public_assets,
        )
        for check in checks:
            fetch_public_url(check, curl_bin=args.curl)
        checksum_sources = verify_checksum_contents(public_assets, curl_bin=args.curl)
        provenance_result = verify_provenance_contents(
            public_assets,
            repository=repository,
            release_tag=resolution.resolved_tag,
            curl_bin=args.curl,
        )
        bundle_metadata = fetch_bundle_metadata(public_assets[METADATA_NAME], curl_bin=args.curl)
        tag_agreement = verify_tag_agreement(
            latest_tag=release_tag_from_metadata_for_assets(latest.release),
            stable_bootstrap_tag=resolution.resolved_tag,
            pinned_install_url=release_root.pinned_install_url(resolution.resolved_tag),
            bundle_metadata=bundle_metadata,
            latest_source_mode=resolution.latest_source_mode,
            guard_source_mode=resolution.guard_source_mode,
        )
        fixture_install_result = (
            run_hostless_install_fixture(
                install_url=release_root.latest_install_url,
                bundle_url=resolution.pinned_asset_urls[BUNDLE_NAME],
                release_tag=resolution.resolved_tag,
                curl_bin=args.curl,
                fixture_root=args.hostless_fixture_root,
                use_sudo=args.hostless_install_sudo,
            )
            if args.hostless_install_fixture
            else None
        )
        proof = freshness_proof_json(
            resolution,
            checks,
            public_assets,
            checksum_sources,
            tag_agreement=tag_agreement,
            fixture_install_result=fixture_install_result,
            provenance_result=provenance_result,
            command_inventory_summary=command_inventory_summary,
            generated_at=generated_at,
        )
    except ValueError as exc:
        message = str(exc)
        failure_class = classify_freshness_exception(message)
        policy_message = freshness_failure_policy_message(failure_class, message)
        if command_inventory_summary is None:
            command_inventory_summary = unavailable_command_inventory(message)
        if args.proof_out is not None:
            proof = freshness_failure_proof_json(
                repository=repository,
                generated_at=generated_at,
                failure_class=failure_class,
                failure_message=policy_message,
                resolved_tag=resolved_tag,
                checks=checks,
                public_assets=public_assets,
                checksum_sources=checksum_sources,
                command_inventory_summary=command_inventory_summary,
                latest_source_mode=latest_source_mode,
                guard_source_mode=guard_source_mode,
            )
            write_proof(args.proof_out, proof)
        raise SystemExit(policy_message) from exc

    if args.proof_out is not None:
        write_proof(args.proof_out, proof)
    if args.json:
        print(json.dumps(proof, indent=2, sort_keys=True))
    else:
        print(
            "freshness network bounded: "
            f"repository={resolution.repository} tag={resolution.resolved_tag} checked_urls={len(checks)}"
        )
    return 0


def write_proof(path: Path, proof: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")


def load_freshness_guard_source(args: argparse.Namespace, latest):
    if args.guard_metadata is not None:
        return load_latest_source(
            args.guard_metadata,
            None,
            curl_bin=args.curl,
            label="guard latest release metadata",
        )
    if args.guard_latest_url is not None:
        return load_latest_source(
            None,
            args.guard_latest_url,
            curl_bin=args.curl,
            label="guard latest release metadata",
        )
    if args.latest_metadata is not None:
        return latest
    return load_latest_source(
        None,
        args.latest_url,
        curl_bin=args.curl,
        label="guard latest release metadata",
    )


def freshness_urls(
    resolved_tag: str,
    pinned_asset_urls: dict[str, str],
    *,
    command_inventory,
    public_assets: dict[str, FreshnessAsset],
) -> list[FreshnessUrl]:
    release_root = public_release_root()
    by_url: dict[str, FreshnessUrl] = {}
    install_asset = public_assets["install.sh"]
    add_freshness_url(
        by_url,
        role="latest-install",
        url=release_root.latest_install_url,
        asset_name="install.sh",
        release_tag="latest",
        source="release-url-contract:latest-install",
        size_bytes=install_asset.size_bytes,
        sha256=install_asset.sha256,
    )
    add_freshness_url(
        by_url,
        role="pinned-install",
        url=release_root.pinned_install_url(resolved_tag),
        asset_name="install.sh",
        release_tag=resolved_tag,
        source="release-url-contract:pinned-install",
        size_bytes=install_asset.size_bytes,
        sha256=install_asset.sha256,
    )
    for asset_name, url in sorted(pinned_asset_urls.items()):
        asset = public_assets[asset_name]
        add_freshness_url(
            by_url,
            role=asset.role,
            url=url,
            asset_name=asset_name,
            release_tag=resolved_tag,
            source="stable-latest-bootstrap:pinned-assets",
            size_bytes=asset.size_bytes,
            sha256=asset.sha256,
        )
    add_docs_linked_urls(
        by_url,
        resolved_tag,
        command_inventory=command_inventory,
        public_assets=public_assets,
    )
    return sorted(by_url.values(), key=freshness_url_sort_key)


def add_docs_linked_urls(
    by_url: dict[str, FreshnessUrl],
    resolved_tag: str,
    *,
    command_inventory,
    public_assets: dict[str, FreshnessAsset],
) -> None:
    install_asset = public_assets["install.sh"]
    for snippet in command_inventory:
        for match in INSTALL_URL_RE.finditer(snippet.body):
            url = match.group(0)
            if "<version>" in url:
                continue
            tag = release_tag_from_install_url(url, fallback=resolved_tag)
            add_freshness_url(
                by_url,
                role="docs-linked-install",
                url=url,
                asset_name="install.sh",
                release_tag=tag,
                source=f"docs:{snippet.path}:{snippet.line}",
                size_bytes=install_asset.size_bytes,
                sha256=install_asset.sha256,
            )


def freshness_url_sort_key(check: FreshnessUrl) -> tuple:
    return (check.role, check.release_tag, check.asset_name, check.url, check.sources)


def release_tag_from_install_url(url: str, *, fallback: str) -> str:
    latest_marker = "/releases/latest/download/"
    if latest_marker in url:
        return "latest"
    pinned_marker = "/releases/download/"
    if pinned_marker not in url:
        return fallback
    suffix = url.split(pinned_marker, 1)[1]
    tag, sep, _asset = suffix.partition("/")
    return tag if sep and tag else fallback


def add_freshness_url(
    by_url: dict[str, FreshnessUrl],
    *,
    role: str,
    url: str,
    asset_name: str,
    release_tag: str,
    source: str,
    size_bytes: int | None,
    sha256: str | None,
) -> None:
    current = by_url.get(url)
    if current is None:
        by_url[url] = FreshnessUrl(
            role=role,
            url=url,
            asset_name=asset_name,
            release_tag=release_tag,
            sources=(source,),
            size_bytes=size_bytes,
            sha256=sha256,
        )
        return
    if source in current.sources:
        return
    by_url[url] = FreshnessUrl(
        role=current.role,
        url=current.url,
        asset_name=current.asset_name,
        release_tag=current.release_tag,
        sources=tuple(sorted((*current.sources, source))),
        size_bytes=current.size_bytes,
        sha256=current.sha256,
    )


def public_release_assets(release: dict, *, asset_index: dict | None) -> dict[str, FreshnessAsset]:
    tag = release_tag_from_metadata_for_assets(release)
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise ValueError("release metadata assets must be a list")
    by_name: dict[str, dict] = {}
    for asset in assets:
        if not isinstance(asset, dict):
            raise ValueError("release metadata asset must be an object")
        name = asset.get("name")
        if not isinstance(name, str) or not name:
            raise ValueError("release metadata asset name must be a nonempty string")
        if name in by_name:
            raise ValueError(
                f"release metadata duplicate asset: {name} role={public_asset_role(name)} release_tag={tag}"
            )
        by_name[name] = asset

    missing = [name for name in REQUIRED_PUBLIC_ASSETS if name not in by_name]
    if missing:
        detail = "; ".join(public_asset_failure_detail(tag, name) for name in missing)
        raise ValueError(f"freshness public asset missing: {detail}")

    expectations = asset_index_expectations(asset_index, tag) if asset_index is not None else {}
    result: dict[str, FreshnessAsset] = {}
    for name in REQUIRED_PUBLIC_ASSETS:
        asset = by_name[name]
        role = public_asset_role(name)
        url = required_asset_url(asset, name, tag, role)
        size = optional_asset_size(asset, name, tag, role)
        sha = required_asset_sha(asset, name, tag, role, url)
        expected = expectations.get(name)
        if expected is not None:
            expected_sha, expected_size = expected
            if expected_sha is not None and sha != expected_sha:
                raise ValueError(
                    "freshness public asset digest mismatch: "
                    f"role={role} asset={name} url={url} release_tag={tag} expected_sha256={expected_sha} got_sha256={sha}"
                )
            if expected_size is not None and size is not None and size != expected_size:
                raise ValueError(
                    "freshness public asset size mismatch: "
                    f"role={role} asset={name} url={url} release_tag={tag} expected_size={expected_size} got_size={size}"
                )
        result[name] = FreshnessAsset(
            name=name,
            role=role,
            url=url,
            release_tag=tag,
            size_bytes=size,
            sha256=sha,
        )
    return result


def release_tag_from_metadata_for_assets(release: dict) -> str:
    if not isinstance(release, dict):
        raise ValueError("release metadata must be a JSON object")
    tag = release.get("tag_name")
    if not isinstance(tag, str) or not tag:
        raise ValueError("release metadata tag_name must be a nonempty string")
    return tag


def public_asset_failure_detail(tag: str, name: str) -> str:
    return f"role={public_asset_role(name)} asset={name} url={release_asset_url(tag, name)} release_tag={tag}"


def required_asset_url(asset: dict, name: str, tag: str, role: str) -> str:
    expected = release_asset_url(tag, name)
    url = asset.get("browser_download_url")
    if not isinstance(url, str) or not url:
        raise ValueError(
            "freshness public asset URL missing: "
            f"role={role} asset={name} url={expected} release_tag={tag}"
        )
    if url != expected:
        raise ValueError(
            "freshness public asset URL mismatch: "
            f"role={role} asset={name} expected_url={expected} got_url={url} release_tag={tag}"
        )
    return url


def optional_asset_size(asset: dict, name: str, tag: str, role: str) -> int | None:
    value = asset.get("size")
    if value is None:
        return None
    if not isinstance(value, int) or value < 0:
        raise ValueError(
            "freshness public asset size invalid: "
            f"role={role} asset={name} url={release_asset_url(tag, name)} release_tag={tag} size={value!r}"
        )
    return value


def required_asset_sha(asset: dict, name: str, tag: str, role: str, url: str) -> str:
    digest = asset.get("digest")
    if not isinstance(digest, str) or not digest:
        raise ValueError(
            "freshness public asset digest missing: "
            f"role={role} asset={name} url={url} release_tag={tag}"
        )
    match = SHA256_DIGEST_RE.fullmatch(digest)
    if match is None:
        raise ValueError(
            "freshness public asset digest invalid: "
            f"role={role} asset={name} url={url} release_tag={tag} digest={digest!r}"
        )
    return match.group(1)


def asset_index_expectations(asset_index: dict, tag: str) -> dict[str, tuple[str | None, int | None]]:
    if not isinstance(asset_index, dict):
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, 'm80-release-assets.json')} field=root"
        )
    if asset_index.get("schema_version") != SCHEMA_VERSION:
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, 'm80-release-assets.json')} field=schema_version"
        )
    if asset_index.get("release_tag") != tag:
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, 'm80-release-assets.json')} field=release_tag"
        )
    assets = asset_index.get("assets")
    if not isinstance(assets, list):
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, 'm80-release-assets.json')} field=assets"
        )
    default_rows = [
        asset
        for asset in assets
        if isinstance(asset, dict) and asset.get("name") == BUNDLE_NAME
    ]
    if len(default_rows) != 1:
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, BUNDLE_NAME)} default_bundle_rows={len(default_rows)}"
        )
    default = default_rows[0]
    require_index_equals(default, "url", release_asset_url(tag, BUNDLE_NAME), BUNDLE_NAME, tag)
    require_index_equals(default, "metadata_name", METADATA_NAME, METADATA_NAME, tag)
    require_index_equals(default, "checksum_name", f"{BUNDLE_NAME}.sha256", f"{BUNDLE_NAME}.sha256", tag)
    require_index_equals(
        default,
        "attestation_name",
        INTEGRITY_ATTESTATION_BUNDLE_NAME,
        INTEGRITY_ATTESTATION_BUNDLE_NAME,
        tag,
    )
    bundle_sha = require_index_sha(default, "sha256", BUNDLE_NAME, tag)
    metadata_sha = require_index_sha(default, "metadata_sha256", METADATA_NAME, tag)
    size = default.get("size_bytes")
    if not isinstance(size, int) or size <= 0:
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, BUNDLE_NAME)} field=size_bytes"
        )
    return {
        BUNDLE_NAME: (bundle_sha, size),
        METADATA_NAME: (metadata_sha, None),
    }


def require_index_equals(row: dict, field: str, expected: str, asset_name: str, tag: str) -> None:
    actual = row.get(field)
    if actual != expected:
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, asset_name)} field={field} expected={expected} got={actual!r}"
        )


def require_index_sha(row: dict, field: str, asset_name: str, tag: str) -> str:
    value = row.get(field)
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise ValueError(
            "freshness asset-index malformed: "
            f"{public_asset_failure_detail(tag, asset_name)} field={field}"
        )
    return value


def fetch_public_url(check: FreshnessUrl, *, curl_bin: str) -> None:
    try:
        result = subprocess.run(
            public_url_curl_args(curl_bin, check.url),
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise ValueError(
            freshness_failure_message(check, "curl_spawn_failed", None, f"failed to spawn curl: {exc}")
        ) from exc
    if result.returncode == 0:
        return
    detail = result.stderr.strip() or result.stdout.strip() or "no curl output"
    raise ValueError(
        freshness_failure_message(
            check,
            curl_failure_kind(result.returncode),
            result.returncode,
            f"curl exited {result.returncode}: {detail}",
        )
    )


def fetch_checksum_text(asset: FreshnessAsset, *, curl_bin: str) -> str:
    return fetch_public_asset_text(asset, curl_bin=curl_bin, source="checksum-content")


def fetch_public_asset_text(asset: FreshnessAsset, *, curl_bin: str, source: str) -> str:
    check = FreshnessUrl(
        role=asset.role,
        url=asset.url,
        asset_name=asset.name,
        release_tag=asset.release_tag,
        sources=(source,),
        size_bytes=asset.size_bytes,
        sha256=asset.sha256,
    )
    try:
        result = subprocess.run(
            public_url_text_curl_args(curl_bin, asset.url),
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise ValueError(
            freshness_failure_message(check, "curl_spawn_failed", None, f"failed to spawn curl: {exc}")
        ) from exc
    if result.returncode == 0:
        return result.stdout
    detail = result.stderr.strip() or result.stdout.strip() or "no curl output"
    raise ValueError(
        freshness_failure_message(
            check,
            curl_failure_kind(result.returncode),
            result.returncode,
            f"curl exited {result.returncode}: {detail}",
        )
    )


def fetch_bundle_metadata(asset: FreshnessAsset, *, curl_bin: str) -> dict:
    text = fetch_public_asset_text(asset, curl_bin=curl_bin, source="bundle-metadata-content")
    try:
        value = json.loads(text)
    except json.JSONDecodeError as exc:
        raise ValueError(
            "freshness bundle metadata malformed: "
            f"role={asset.role} asset={asset.name} url={asset.url} release_tag={asset.release_tag} field=root; {exc}"
        ) from exc
    if not isinstance(value, dict):
        raise ValueError(
            "freshness bundle metadata malformed: "
            f"role={asset.role} asset={asset.name} url={asset.url} release_tag={asset.release_tag} field=root"
        )
    return value


def verify_provenance_contents(
    public_assets: dict[str, FreshnessAsset],
    *,
    repository: str,
    release_tag: str,
    curl_bin: str,
) -> dict:
    predicate_asset = public_assets["m80-release-integrity.json"]
    attestation_asset = public_assets[INTEGRITY_ATTESTATION_BUNDLE_NAME]
    predicate = fetch_json_public_asset(predicate_asset, curl_bin=curl_bin)
    subjects = integrity_subjects(predicate, predicate_asset=predicate_asset)

    require_provenance_field(predicate, "schema_version", SCHEMA_VERSION, predicate_asset)
    require_provenance_field(predicate, "repository", repository, predicate_asset)
    require_provenance_field(predicate, "release_tag", release_tag, predicate_asset)
    require_provenance_field(predicate, "bundle_metadata_name", METADATA_NAME, predicate_asset)
    require_provenance_field(predicate, "bundle_metadata_sha256", public_assets[METADATA_NAME].sha256, predicate_asset)
    for name in [BUNDLE_NAME, METADATA_NAME]:
        require_subject_digest(subjects, name, public_assets[name])

    attestation = parse_attestation_bundle(
        fetch_public_asset_text(attestation_asset, curl_bin=curl_bin, source="attestation-content"),
        attestation_asset=attestation_asset,
    )
    require_attestation_subject(attestation["subjects"], predicate_asset)
    return {
        "status": "success",
        "release_tag": release_tag,
        "predicate": {
            "name": predicate_asset.name,
            "url": predicate_asset.url,
            "sha256": predicate_asset.sha256,
        },
        "attestation_bundle": {
            "name": attestation_asset.name,
            "url": attestation_asset.url,
            "sha256": attestation_asset.sha256,
            "statement_count": len(attestation["statements"]),
        },
        "subjects": [
            subject_summary(subjects[name], url=public_assets[name].url)
            for name in [BUNDLE_NAME, METADATA_NAME]
        ],
        "attestation_subjects": attestation["subjects"],
    }


def fetch_json_public_asset(asset: FreshnessAsset, *, curl_bin: str) -> dict:
    text = fetch_public_asset_text(asset, curl_bin=curl_bin, source=f"{asset.role}-content")
    try:
        value = json.loads(text)
    except json.JSONDecodeError as exc:
        raise ValueError(
            "freshness provenance malformed: "
            f"role={asset.role} asset={asset.name} url={asset.url} release_tag={asset.release_tag} "
            f"field=root expected=json-object got=json-error:{exc.msg}"
        ) from exc
    if not isinstance(value, dict):
        raise ValueError(
            "freshness provenance malformed: "
            f"role={asset.role} asset={asset.name} url={asset.url} release_tag={asset.release_tag} "
            f"field=root expected=json-object got={type(value).__name__}"
        )
    return value


def integrity_subjects(predicate: dict, *, predicate_asset: FreshnessAsset) -> dict[str, dict]:
    subjects = predicate.get("subjects")
    if not isinstance(subjects, list):
        raise ValueError(
            "freshness provenance malformed: "
            f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
            f"release_tag={predicate_asset.release_tag} field=subjects expected=list got={type(subjects).__name__}"
        )
    result = {}
    for subject in subjects:
        if not isinstance(subject, dict):
            raise ValueError(
                "freshness provenance malformed: "
                f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
                f"release_tag={predicate_asset.release_tag} field=subjects[] expected=object"
            )
        name = subject.get("name")
        if not isinstance(name, str) or not name:
            raise ValueError(
                "freshness provenance malformed: "
                f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
                f"release_tag={predicate_asset.release_tag} field=subjects[].name expected=asset-name got={name!r}"
            )
        if name in result:
            raise ValueError(
                "freshness provenance malformed: "
                f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
                f"release_tag={predicate_asset.release_tag} field=subjects duplicate={name}"
            )
        result[name] = subject
    return result


def require_provenance_field(predicate: dict, field: str, expected, predicate_asset: FreshnessAsset) -> None:
    got = predicate.get(field)
    if got != expected:
        raise ValueError(
            "freshness provenance mismatch: "
            f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
            f"release_tag={predicate_asset.release_tag} field={field} expected={expected!r} got={got!r}"
        )


def require_subject_digest(subjects: dict[str, dict], name: str, asset: FreshnessAsset) -> None:
    subject = subjects.get(name)
    if subject is None:
        raise ValueError(
            "freshness provenance mismatch: "
            f"role={asset.role} asset={name} url={asset.url} release_tag={asset.release_tag} "
            f"field=subjects expected=present got=missing"
        )
    got = subject.get("sha256")
    if got != asset.sha256:
        raise ValueError(
            "freshness provenance mismatch: "
            f"role={asset.role} asset={name} url={asset.url} release_tag={asset.release_tag} "
            f"field=subjects.sha256 expected={asset.sha256} got={got!r}"
        )


def subject_summary(subject: dict, *, url: str) -> dict:
    return {
        "name": subject["name"],
        "kind": subject.get("kind"),
        "sha256": subject["sha256"],
        "size_bytes": subject.get("size_bytes"),
        "url": url,
    }


def parse_attestation_bundle(text: str, *, attestation_asset: FreshnessAsset) -> dict:
    statements = []
    subjects = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            bundle = json.loads(line)
        except json.JSONDecodeError as exc:
            raise ValueError(
                "freshness attestation malformed: "
                f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
                f"release_tag={attestation_asset.release_tag} field=jsonl line={line_number} "
                f"expected=json-object got=json-error:{exc.msg}"
            ) from exc
        if not isinstance(bundle, dict):
            raise ValueError(
                "freshness attestation malformed: "
                f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
                f"release_tag={attestation_asset.release_tag} field=jsonl line={line_number} expected=object"
            )
        statement = attestation_statement(bundle, attestation_asset=attestation_asset, line_number=line_number)
        statements.append(statement)
        subjects.extend(statement_subjects(statement, attestation_asset=attestation_asset))
    if not statements:
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=jsonl expected=nonempty"
        )
    return {"statements": statements, "subjects": subjects}


def attestation_statement(bundle: dict, *, attestation_asset: FreshnessAsset, line_number: int) -> dict:
    envelope = bundle.get("dsseEnvelope")
    if not isinstance(envelope, dict):
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=dsseEnvelope line={line_number} expected=object"
        )
    payload = envelope.get("payload")
    if not isinstance(payload, str):
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=dsseEnvelope.payload line={line_number} expected=base64"
        )
    try:
        decoded = base64.b64decode(payload, validate=True)
        statement = json.loads(decoded)
    except (binascii.Error, json.JSONDecodeError, UnicodeDecodeError) as exc:
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=dsseEnvelope.payload line={line_number} "
            "expected=in-toto-json"
        ) from exc
    if not isinstance(statement, dict):
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=statement line={line_number} expected=object"
        )
    workflow = statement.get("predicate", {}).get("buildDefinition", {}).get("externalParameters", {}).get("workflow", {})
    if not isinstance(workflow, dict):
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=workflow expected=object got={workflow!r}"
        )
    expected_repository = f"https://github.com/{public_release_root().repository}"
    if workflow.get("repository") != expected_repository:
        raise ValueError(
            "freshness attestation mismatch: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=workflow.repository "
            f"expected={expected_repository} got={workflow.get('repository')!r}"
        )
    expected_ref = f"refs/tags/{attestation_asset.release_tag}"
    if workflow.get("ref") != expected_ref:
        raise ValueError(
            "freshness attestation mismatch: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=workflow.ref expected={expected_ref} "
            f"got={workflow.get('ref')!r}"
        )
    return statement


def statement_subjects(statement: dict, *, attestation_asset: FreshnessAsset) -> list[dict]:
    subjects = statement.get("subject")
    if not isinstance(subjects, list):
        raise ValueError(
            "freshness attestation malformed: "
            f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
            f"release_tag={attestation_asset.release_tag} field=subject expected=list"
        )
    result = []
    for subject in subjects:
        if not isinstance(subject, dict):
            raise ValueError(
                "freshness attestation malformed: "
                f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
                f"release_tag={attestation_asset.release_tag} field=subject[] expected=object"
            )
        name = subject.get("name")
        digest = subject.get("digest")
        sha256 = digest.get("sha256") if isinstance(digest, dict) else None
        if not isinstance(name, str) or SHA256_RE.fullmatch(str(sha256 or "")) is None:
            raise ValueError(
                "freshness attestation malformed: "
                f"role={attestation_asset.role} asset={attestation_asset.name} url={attestation_asset.url} "
                f"release_tag={attestation_asset.release_tag} field=subject[] expected=name+sha256"
            )
        result.append({"name": name, "sha256": sha256, "url": attestation_asset.url})
    return result


def require_attestation_subject(subjects: list[dict], predicate_asset: FreshnessAsset) -> None:
    for subject in subjects:
        if subject.get("name") == predicate_asset.name:
            got = subject.get("sha256")
            if got == predicate_asset.sha256:
                return
            raise ValueError(
                "freshness attestation mismatch: "
                f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
                f"release_tag={predicate_asset.release_tag} field=subject.sha256 "
                f"expected={predicate_asset.sha256} got={got!r}"
            )
    raise ValueError(
        "freshness attestation mismatch: "
        f"role={predicate_asset.role} asset={predicate_asset.name} url={predicate_asset.url} "
        f"release_tag={predicate_asset.release_tag} field=subject expected=present got=missing"
    )


def public_url_curl_args(curl_bin: str, url: str) -> list[str]:
    args = public_url_text_curl_args(curl_bin, url)
    return [
        *args[:-1],
        "--output",
        "/dev/null",
        args[-1],
    ]


def public_url_text_curl_args(curl_bin: str, url: str) -> list[str]:
    return [
        curl_bin,
        "-fsSL",
        "--connect-timeout",
        str(FETCH_CONNECT_TIMEOUT_SECONDS),
        "--max-time",
        str(FETCH_MAX_TIME_SECONDS),
        "--retry",
        str(FETCH_RETRY_COUNT),
        "--retry-delay",
        str(FETCH_RETRY_DELAY_SECONDS),
        url,
    ]


def curl_failure_kind(status: int) -> str:
    if status in {6, 7}:
        return "dns_or_connect_failure"
    if status == 18:
        return "partial_download"
    if status == 22:
        return "http_failure"
    if status == 28:
        return "timeout"
    if status == 47:
        return "redirect_loop"
    if status == 130:
        return "interrupted"
    return "public_url_fetch_failure"


def freshness_failure_message(
    check: FreshnessUrl,
    failure: str,
    curl_exit: int | None,
    detail: str,
) -> str:
    failure_class = classify_public_url_failure(check, failure)
    fields = [
        "freshness public URL fetch failed",
        f"failure_class={failure_class}",
        f"failure={failure}",
        f"url={check.url}",
        f"release_tag={check.release_tag}",
        f"asset={check.asset_name}",
        f"role={check.role}",
        f"sources={','.join(check.sources)}",
    ]
    repair_command = freshness_repair_command(failure_class)
    if repair_command:
        fields.append(f"repair_command={repair_command}")
    if curl_exit is not None:
        fields.append(f"curl_exit={curl_exit}")
    fields.append(detail)
    return "; ".join(fields)


def classify_public_url_failure(check: FreshnessUrl, failure: str) -> str:
    if failure in NETWORK_FAILURE_KINDS and failure != "http_failure":
        return "network-transient"
    if failure == "http_failure":
        if any(source.startswith("docs:") for source in check.sources) and not any(
            source.startswith(("release-url-contract:", "stable-latest-bootstrap:"))
            for source in check.sources
        ):
            return "docs-drift"
        return "missing-public-asset"
    return "verifier-schema-drift"


def classify_freshness_exception(message: str) -> str:
    if "failure_class=" in message:
        match = re.search(r"\bfailure_class=([A-Za-z0-9-]+)", message)
        return match.group(1) if match is not None else "verifier-schema-drift"
    if any(f"failure={failure}" in message for failure in NETWORK_FAILURE_KINDS):
        return "network-transient"
    if "failure=latest_tag_switch" in message or "latest tag" in message or "freshness tag agreement mismatch" in message:
        return "stale-latest"
    if "freshness public asset missing" in message:
        return "missing-public-asset"
    if "checked_command_inventory_digest" in message:
        return "docs-drift"
    if "digest mismatch" in message or "checksum" in message:
        return "checksum-mismatch"
    if "freshness provenance" in message or "freshness attestation" in message:
        return "provenance-mismatch"
    if "URL mismatch" in message or "size mismatch" in message:
        return "provenance-mismatch"
    if (
        "stable release ineligible" in message
        or "stable release tag must be" in message
        or "release metadata tag mismatch" in message
    ):
        return "public-release-unavailable"
    return "verifier-schema-drift"


def freshness_failure_policy_message(failure_class: str, message: str) -> str:
    if "failure_class=" in message:
        return message
    fields = [f"failure_class={failure_class}"]
    repair_command = freshness_repair_command(failure_class)
    if repair_command:
        fields.append(f"repair_command={repair_command}")
    fields.append(message)
    return "; ".join(fields)


def freshness_repair_command(failure_class: str) -> str | None:
    if not failure_class:
        return None
    try:
        policy = read_json(
            Path(__file__).resolve().parents[1] / "docs" / "behaviors" / "release" / "freshness-failure-policy.json",
            "freshness failure policy",
        )
    except (SystemExit, ValueError):
        return None
    for row in policy.get("failure_classes", []):
        if isinstance(row, dict) and row.get("id") == failure_class:
            command = row.get("repair_command")
            return command if isinstance(command, str) and command else None
    return None


def verify_checksum_contents(
    public_assets: dict[str, FreshnessAsset],
    *,
    curl_bin: str,
) -> dict[str, list[str]]:
    sources = {name: ["github-release-metadata"] for name in public_assets}
    sums_asset = public_assets["SHA256SUMS"]
    sums = fetch_checksum_text(sums_asset, curl_bin=curl_bin)
    for asset_name, digest in parse_checksum_file(sums, checksum_asset=sums_asset).items():
        asset = public_assets.get(asset_name)
        if asset is None:
            raise ValueError(
                checksum_failure_message(
                    "unknown asset in SHA256SUMS",
                    checksum_asset=sums_asset,
                    asset_name=asset_name,
                    asset=FreshnessAsset(
                        name=asset_name,
                        role=public_asset_role(asset_name),
                        url=release_asset_url(sums_asset.release_tag, asset_name),
                        release_tag=sums_asset.release_tag,
                        size_bytes=None,
                        sha256="unknown",
                    ),
                )
            )
        if digest != asset.sha256:
            raise ValueError(
                checksum_failure_message(
                    "SHA256SUMS digest mismatch",
                    checksum_asset=sums_asset,
                    asset_name=asset_name,
                    asset=asset,
                    expected=asset.sha256,
                    got=digest,
                )
            )
        sources[asset_name].append("SHA256SUMS")

    for checksum_asset in sorted(
        (asset for asset in public_assets.values() if asset.name.endswith(".sha256")),
        key=lambda asset: asset.name,
    ):
        target_name = checksum_asset.name.removesuffix(".sha256")
        target = public_assets.get(target_name)
        if target is None:
            raise ValueError(
                checksum_failure_message(
                    "checksum sidecar target missing",
                    checksum_asset=checksum_asset,
                    asset_name=target_name,
                    asset=FreshnessAsset(
                        name=target_name,
                        role=public_asset_role(target_name),
                        url=release_asset_url(checksum_asset.release_tag, target_name),
                        release_tag=checksum_asset.release_tag,
                        size_bytes=None,
                        sha256="unknown",
                    ),
                )
            )
        entries = parse_checksum_file(fetch_checksum_text(checksum_asset, curl_bin=curl_bin), checksum_asset=checksum_asset)
        if len(entries) != 1:
            raise ValueError(
                checksum_failure_message(
                    "checksum sidecar must contain exactly one entry",
                    checksum_asset=checksum_asset,
                    asset_name=target_name,
                    asset=target,
                    got=str(len(entries)),
                )
            )
        actual_name, digest = next(iter(entries.items()))
        if actual_name != target_name:
            raise ValueError(
                checksum_failure_message(
                    "checksum sidecar asset-name mismatch",
                    checksum_asset=checksum_asset,
                    asset_name=target_name,
                    asset=target,
                    expected=target_name,
                    got=actual_name,
                )
            )
        if digest != target.sha256:
            raise ValueError(
                checksum_failure_message(
                    "checksum sidecar digest mismatch",
                    checksum_asset=checksum_asset,
                    asset_name=target_name,
                    asset=target,
                    expected=target.sha256,
                    got=digest,
                )
            )
        sources[target_name].append(checksum_asset.name)
    return {name: sorted(values) for name, values in sources.items()}


def verify_tag_agreement(
    *,
    latest_tag: str,
    stable_bootstrap_tag: str,
    pinned_install_url: str,
    bundle_metadata: dict,
    latest_source_mode: str,
    guard_source_mode: str,
) -> dict:
    pinned_url_tag = release_tag_from_install_url(pinned_install_url, fallback="")
    metadata_release_tag = require_bundle_metadata_string(bundle_metadata, "release_tag", stable_bootstrap_tag)
    metadata_m80_version = require_bundle_metadata_string(bundle_metadata, "m80_version", stable_bootstrap_tag)
    metadata_package_version = require_bundle_metadata_string(bundle_metadata, "package_version", stable_bootstrap_tag)
    expected_package_version = stable_bootstrap_tag.removeprefix("v")
    comparisons = (
        ("latest_vs_stable_bootstrap", stable_bootstrap_tag, latest_tag),
        ("stable_bootstrap_vs_pinned_install_url", stable_bootstrap_tag, pinned_url_tag),
        ("stable_bootstrap_vs_bundle_metadata_release_tag", stable_bootstrap_tag, metadata_release_tag),
        ("stable_bootstrap_vs_bundle_metadata_m80_version", stable_bootstrap_tag, metadata_m80_version),
        ("stable_bootstrap_package_vs_bundle_metadata_package_version", expected_package_version, metadata_package_version),
    )
    for pair, expected, got in comparisons:
        if expected != got:
            raise ValueError(
                "freshness tag agreement mismatch: "
                f"pair={pair}; expected={expected}; got={got}; "
                f"stable_bootstrap_tag={stable_bootstrap_tag}; "
                f"pinned_install_command={pinned_install_command(stable_bootstrap_tag)}"
            )
    return {
        "status": "success",
        "latest_tag": latest_tag,
        "stable_bootstrap_tag": stable_bootstrap_tag,
        "guard_tag": stable_bootstrap_tag,
        "latest_install_url": public_release_root().latest_install_url,
        "pinned_install_url": pinned_install_url,
        "pinned_install_url_tag": pinned_url_tag,
        "bundle_metadata_release_tag": metadata_release_tag,
        "bundle_metadata_m80_version": metadata_m80_version,
        "bundle_metadata_package_version": metadata_package_version,
        "latest_source_mode": latest_source_mode,
        "guard_source_mode": guard_source_mode,
    }


def require_bundle_metadata_string(metadata: dict, field: str, stable_bootstrap_tag: str) -> str:
    value = metadata.get(field)
    if isinstance(value, str) and value:
        return value
    raise ValueError(
        "freshness tag agreement mismatch: "
        f"pair=stable_bootstrap_vs_bundle_metadata_{field}; expected={stable_bootstrap_tag}; got={value!r}; "
        f"stable_bootstrap_tag={stable_bootstrap_tag}; "
        f"pinned_install_command={pinned_install_command(stable_bootstrap_tag)}"
    )


def parse_checksum_file(text: str, *, checksum_asset: FreshnessAsset) -> dict[str, str]:
    entries: dict[str, str] = {}
    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2 or SHA256_RE.fullmatch(parts[0]) is None:
            raise ValueError(
                checksum_failure_message(
                    "checksum line malformed",
                    checksum_asset=checksum_asset,
                    asset_name=checksum_asset.name,
                    asset=checksum_asset,
                    got=f"line {line_number}",
                )
            )
        digest, asset_name = parts
        asset_name = asset_name.removeprefix("*")
        if not asset_name or "/" in asset_name or "\\" in asset_name:
            raise ValueError(
                checksum_failure_message(
                    "checksum asset-name malformed",
                    checksum_asset=checksum_asset,
                    asset_name=asset_name or checksum_asset.name,
                    asset=checksum_asset,
                    got=f"line {line_number}: {asset_name!r}",
                )
            )
        if asset_name in entries:
            raise ValueError(
                checksum_failure_message(
                    "duplicate checksum entry",
                    checksum_asset=checksum_asset,
                    asset_name=asset_name,
                    asset=FreshnessAsset(
                        name=asset_name,
                        role=public_asset_role(asset_name),
                        url=release_asset_url(checksum_asset.release_tag, asset_name),
                        release_tag=checksum_asset.release_tag,
                        size_bytes=None,
                        sha256="duplicate",
                    ),
                )
            )
        entries[asset_name] = digest
    if not entries:
        raise ValueError(
            checksum_failure_message(
                "checksum file empty",
                checksum_asset=checksum_asset,
                asset_name=checksum_asset.name,
                asset=checksum_asset,
            )
        )
    return entries


def checksum_failure_message(
    reason: str,
    *,
    checksum_asset: FreshnessAsset,
    asset_name: str,
    asset: FreshnessAsset,
    expected: str | None = None,
    got: str | None = None,
) -> str:
    fields = [
        f"freshness checksum content mismatch: {reason}",
        f"role={asset.role}",
        f"asset={asset_name}",
        f"checksum_asset={checksum_asset.name}",
        f"checksum_url={checksum_asset.url}",
        f"asset_url={asset.url}",
        f"release_tag={checksum_asset.release_tag}",
    ]
    if expected is not None:
        fields.append(f"expected={expected}")
    if got is not None:
        fields.append(f"got={got}")
    return "; ".join(fields)


def run_hostless_install_fixture(
    *,
    install_url: str,
    bundle_url: str,
    release_tag: str,
    curl_bin: str,
    fixture_root: Path | None = None,
    use_sudo: bool = False,
    watched_host_paths: tuple[Path, ...] = (Path("/opt"), Path("/etc")),
) -> dict:
    root = prepare_hostless_fixture_root(fixture_root)
    cleanup_on_failure = fixture_root is None
    try:
        result = execute_hostless_install_fixture(
            fixture_root=root,
            install_url=install_url,
            bundle_url=bundle_url,
            release_tag=release_tag,
            curl_bin=curl_bin,
            use_sudo=use_sudo,
            watched_host_paths=watched_host_paths,
        )
    except Exception:
        if cleanup_on_failure:
            shutil.rmtree(root, ignore_errors=True)
        raise
    return result | {
        "fixture_root": str(root),
        "cleanup_on_failure": cleanup_on_failure,
        "privilege": "sudo -n" if use_sudo else "current-user",
    }


def prepare_hostless_fixture_root(fixture_root: Path | None) -> Path:
    if fixture_root is not None:
        fixture_root.mkdir(parents=True, exist_ok=True)
        return fixture_root
    parent = Path(os.environ.get("TMPDIR", tempfile.gettempdir()))
    if parent.exists() and os.access(parent, os.W_OK):
        return Path(tempfile.mkdtemp(prefix="m80-freshness-install-", dir=parent))
    return Path(tempfile.mkdtemp(prefix="m80-freshness-install-"))


def execute_hostless_install_fixture(
    *,
    fixture_root: Path,
    install_url: str,
    bundle_url: str,
    release_tag: str,
    curl_bin: str,
    use_sudo: bool,
    watched_host_paths: tuple[Path, ...],
) -> dict:
    fixture_root.mkdir(parents=True, exist_ok=True)
    bin_dir = fixture_root / "bin"
    host_bin_dir = fixture_root / "host-bin"
    install_root = fixture_root / "install-root"
    download_dir = fixture_root / "downloads"
    home_dir = fixture_root / "home"
    tmp_dir = fixture_root / "tmp"
    gh_log = fixture_root / "gh-invocations.log"
    for path in (bin_dir, host_bin_dir, download_dir, home_dir, tmp_dir):
        path.mkdir(parents=True, exist_ok=True)

    installer_path = download_dir / "install.sh"
    download_public_file(install_url, installer_path, curl_bin=curl_bin)
    installer_path.chmod(installer_path.stat().st_mode | 0o700)
    write_hostless_binary_fixtures(host_bin_dir)
    gh_wrapper = write_gh_audit_wrapper(bin_dir / "gh", gh_log)

    before = {str(path): path_state(path) for path in watched_host_paths}
    env = os.environ.copy()
    env.update(
        {
            "HOME": str(home_dir),
            "TMPDIR": str(tmp_dir),
            "PATH": f"{bin_dir}:{env.get('PATH', '')}",
            "M80_INSTALL_HOSTLESS_FIXTURE": "1",
            "M80_FIRECRACKER_BIN": str(host_bin_dir / "firecracker"),
            "M80_FIRECRACKER_SECCOMP_FILTER": str(host_bin_dir / "firecracker-seccomp-filter.bin"),
            "M80_JAILER_BIN": str(host_bin_dir / "jailer"),
            "M80_RELEASE_ATTESTATION_GH": str(gh_wrapper),
            "M80_JAIL_UID": str(os.getuid()),
            "M80_JAIL_GID": str(os.getgid()),
        }
    )
    install_command = ["sh", str(installer_path), "--install-root", str(install_root)]
    command = sudo_env_command(env, install_command) if use_sudo else install_command
    process = subprocess.run(
        command,
        text=True,
        capture_output=True,
        check=False,
        env=None if use_sudo else env,
        timeout=FETCH_MAX_TIME_SECONDS * 3,
    )
    after = {str(path): path_state(path) for path in watched_host_paths}
    gh_invocations = read_gh_invocations(gh_log)
    mutation = host_mutation_result(before, after, gh_invocations)
    if process.returncode != 0:
        raise ValueError(
            "freshness hostless fixture install failed: "
            f"exit={process.returncode}; install_url={install_url}; install_root={install_root}; "
            f"stderr={process.stderr.strip() or '<empty>'}"
        )
    if mutation["status"] != "success":
        raise ValueError(
            "freshness hostless fixture install mutated host state: "
            f"install_root={install_root}; no_host_mutation={json.dumps(mutation, sort_keys=True)}"
        )
    fields = parse_key_value_lines(process.stdout)
    active_version_dir = fields.get("active_version_dir") or require_fixture_field(fields, "version_dir", process.stdout)
    profile_path = require_fixture_field(fields, "profile_path", process.stdout)
    host_binaries_manifest = require_fixture_field(fields, "host_binaries_manifest", process.stdout)
    return {
        "status": "success",
        "mode": "hostless-install-root",
        "release_tag": release_tag,
        "install_url": install_url,
        "bundle_url": bundle_url,
        "command": command,
        "exit_status": process.returncode,
        "install_root": str(install_root),
        "staged_bundle_path": active_version_dir,
        "active_version_dir": active_version_dir,
        "active_profile_path": profile_path,
        "host_binaries_manifest_path": host_binaries_manifest,
        "preflight_gate": fields.get("preflight_gate"),
        "stdout": process.stdout.splitlines(),
        "stderr": process.stderr.splitlines(),
        "no_host_mutation": mutation,
    }


def sudo_env_command(env: dict[str, str], command: list[str]) -> list[str]:
    forwarded = [
        "HOME",
        "TMPDIR",
        "PATH",
        "M80_INSTALL_HOSTLESS_FIXTURE",
        "M80_FIRECRACKER_BIN",
        "M80_FIRECRACKER_SECCOMP_FILTER",
        "M80_JAILER_BIN",
        "M80_RELEASE_ATTESTATION_GH",
        "M80_JAIL_UID",
        "M80_JAIL_GID",
    ]
    return ["sudo", "-n", "env", "-i", *(f"{key}={env[key]}" for key in forwarded), *command]


def download_public_file(url: str, path: Path, *, curl_bin: str) -> None:
    result = subprocess.run(
        [
            curl_bin,
            "-fsSL",
            "--connect-timeout",
            str(FETCH_CONNECT_TIMEOUT_SECONDS),
            "--max-time",
            str(FETCH_MAX_TIME_SECONDS),
            "--retry",
            str(FETCH_RETRY_COUNT),
            "--retry-delay",
            str(FETCH_RETRY_DELAY_SECONDS),
            "--output",
            str(path),
            url,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no curl output"
        raise ValueError(f"freshness hostless fixture installer download failed: url={url}; curl exited {result.returncode}: {detail}")


def write_hostless_binary_fixtures(host_bin_dir: Path) -> None:
    write_executable(
        host_bin_dir / "firecracker",
        "#!/bin/sh\nprintf '%s\\n' 'Firecracker v1.15.1'\n",
    )
    write_executable(
        host_bin_dir / "jailer",
        "#!/bin/sh\nprintf '%s\\n' 'Jailer v1.15.1'\n",
    )
    (host_bin_dir / "firecracker-seccomp-filter.bin").write_text('{"fixture":"hostless"}\n')


def write_gh_audit_wrapper(path: Path, log: Path) -> Path:
    real_gh = shutil.which("gh")
    if real_gh is None:
        raise ValueError("freshness hostless fixture requires gh on PATH for public attestation verification")
    script = f"""#!/usr/bin/env python3
import os
import shlex
import sys
from pathlib import Path

log = Path({str(log)!r})
log.parent.mkdir(parents=True, exist_ok=True)
with log.open("a") as f:
    f.write(" ".join(shlex.quote(arg) for arg in sys.argv[1:]) + "\\n")

def forbidden_gh_write(args):
    if "release" in args:
        index = args.index("release")
        if len(args) > index + 1 and args[index + 1] in {sorted({"upload", "edit", "delete", "create"})!r}:
            return True
    if "api" in args and any("release" in arg for arg in args):
        for index, arg in enumerate(args):
            if arg in ("-X", "--method") and len(args) > index + 1:
                return args[index + 1].upper() in ("POST", "PATCH", "PUT", "DELETE")
            if arg.startswith("--method="):
                return arg.split("=", 1)[1].upper() in ("POST", "PATCH", "PUT", "DELETE")
    return False

if forbidden_gh_write(sys.argv[1:]):
    sys.stderr.write("m80 freshness fixture blocked gh release write API\\n")
    raise SystemExit(97)

os.execv({real_gh!r}, [{real_gh!r}, *sys.argv[1:]])
"""
    write_executable(path, script)
    return path


def write_executable(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(path.stat().st_mode | 0o700)


def path_state(path: Path) -> dict:
    try:
        stat_result = path.lstat()
    except FileNotFoundError:
        return {"exists": False}
    return {
        "exists": True,
        "mode": stat_result.st_mode,
        "size": stat_result.st_size,
        "mtime_ns": stat_result.st_mtime_ns,
        "is_symlink": path.is_symlink(),
    }


def host_mutation_result(before: dict[str, dict], after: dict[str, dict], gh_invocations: list[list[str]]) -> dict:
    changed = sorted(path for path, state in after.items() if state != before[path])
    forbidden = forbidden_gh_write_invocations(gh_invocations)
    return {
        "status": "success" if not changed and not forbidden else "failure",
        "watched_paths_unchanged": not changed,
        "watched_paths": sorted(before),
        "changed_paths": changed,
        "forbidden_gh_write_api_invoked": bool(forbidden),
        "forbidden_gh_invocations": forbidden,
        "gh_invocations": gh_invocations,
    }


def read_gh_invocations(log: Path) -> list[list[str]]:
    if not log.exists():
        return []
    return [shlex.split(line) for line in log.read_text().splitlines() if line.strip()]


def forbidden_gh_write_invocations(invocations: list[list[str]]) -> list[list[str]]:
    forbidden_actions = {"upload", "edit", "delete", "create"}
    return [invocation for invocation in invocations if is_forbidden_gh_write_invocation(invocation, forbidden_actions)]


def is_forbidden_gh_write_invocation(invocation: list[str], forbidden_actions: set[str]) -> bool:
    if "release" in invocation:
        index = invocation.index("release")
        return len(invocation) > index + 1 and invocation[index + 1] in forbidden_actions
    if "api" in invocation and any("release" in arg for arg in invocation):
        return gh_api_method(invocation) in {"POST", "PATCH", "PUT", "DELETE"}
    return False


def gh_api_method(invocation: list[str]) -> str | None:
    for index, arg in enumerate(invocation):
        if arg in {"-X", "--method"} and len(invocation) > index + 1:
            return invocation[index + 1].upper()
        if arg.startswith("--method="):
            return arg.split("=", 1)[1].upper()
    return None


def parse_key_value_lines(text: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in text.splitlines():
        key, sep, value = line.partition("=")
        if sep and key:
            fields[key] = value
    return fields


def require_fixture_field(fields: dict[str, str], name: str, stdout: str) -> str:
    value = fields.get(name)
    if not value:
        raise ValueError(
            "freshness hostless fixture output missing required field: "
            f"field={name}; stdout={stdout.strip() or '<empty>'}"
        )
    return value


def freshness_proof_json(
    resolution,
    checks,
    public_assets,
    checksum_sources,
    *,
    tag_agreement,
    fixture_install_result,
    provenance_result,
    command_inventory_summary,
    generated_at,
) -> dict:
    proof = success_proof(
        repository=resolution.repository,
        resolved_tag=resolution.resolved_tag,
        generated_at=generated_at,
        fetch_policy=freshness_fetch_policy(),
        checked_urls=checked_url_proof_rows(checks),
        public_assets=public_asset_proof_rows(public_assets, checksum_sources),
        public_command_inventory=command_inventory_summary,
        failure_classes=sorted(FRESHNESS_FAILURE_CLASSES),
        latest_source_mode=resolution.latest_source_mode,
        guard_source_mode=resolution.guard_source_mode,
        tag_agreement=tag_agreement,
        fixture_install_result=fixture_install_result,
        provenance_result=provenance_result,
    )
    errors = validate_freshness_proof(proof)
    if errors:
        raise ValueError("freshness proof invalid: " + "; ".join(errors))
    return proof


def freshness_failure_proof_json(
    *,
    repository,
    generated_at,
    failure_class,
    failure_message,
    resolved_tag,
    checks,
    public_assets,
    checksum_sources,
    command_inventory_summary,
    latest_source_mode,
    guard_source_mode,
) -> dict:
    proof = failure_proof(
        repository=repository,
        generated_at=generated_at,
        failure_class=failure_class,
        failure_message=failure_message,
        repair_command=freshness_repair_command(failure_class),
        fetch_policy=freshness_fetch_policy(),
        checked_urls=checked_url_proof_rows(checks),
        public_assets=public_asset_proof_rows(public_assets, checksum_sources),
        public_command_inventory=command_inventory_summary,
        failure_classes=sorted(FRESHNESS_FAILURE_CLASSES),
        latest_source_mode=latest_source_mode,
        guard_source_mode=guard_source_mode,
        resolved_latest_tag=resolved_tag,
    )
    errors = validate_freshness_proof(proof)
    if errors:
        raise ValueError("freshness failure proof invalid: " + "; ".join(errors))
    return proof


def freshness_fetch_policy() -> dict:
    return fetch_policy(
        connect_timeout_seconds=FETCH_CONNECT_TIMEOUT_SECONDS,
        max_time_seconds=FETCH_MAX_TIME_SECONDS,
        retry_count=FETCH_RETRY_COUNT,
        retry_delay_seconds=FETCH_RETRY_DELAY_SECONDS,
    )


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


if __name__ == "__main__":
    raise SystemExit(main())
