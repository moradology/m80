#!/usr/bin/env python3
"""Bounded public release freshness checks for the m80 install surface."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess

from quickstart_snippets import INSTALL_URL_RE, public_command_inventory
from release_url_contract import public_release_root, release_asset_url
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
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    asset_index = read_json(args.asset_index, "release asset index") if args.asset_index else None
    try:
        latest = load_latest_source(
            args.latest_metadata,
            args.latest_url,
            curl_bin=args.curl,
            label="latest release metadata",
        )
        public_assets = public_release_assets(latest.release, asset_index=asset_index)
        guard = load_freshness_guard_source(args, latest)
        resolution = resolve_latest_bootstrap(latest, guard=guard, asset_index=asset_index)
        checks = freshness_urls(
            resolution.resolved_tag,
            resolution.pinned_asset_urls,
            docs_root=args.docs_root,
            public_assets=public_assets,
        )
        for check in checks:
            fetch_public_url(check, curl_bin=args.curl)
        proof = freshness_proof_json(resolution, checks, public_assets)
    except ValueError as exc:
        message = str(exc)
        raise SystemExit(
            freshness_failure_policy_message(classify_freshness_exception(message), message)
        ) from exc

    if args.proof_out is not None:
        args.proof_out.parent.mkdir(parents=True, exist_ok=True)
        args.proof_out.write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")
    if args.json:
        print(json.dumps(proof, indent=2, sort_keys=True))
    else:
        print(
            "freshness network bounded: "
            f"repository={resolution.repository} tag={resolution.resolved_tag} checked_urls={len(checks)}"
        )
    return 0


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
    docs_root: Path,
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
    add_docs_linked_urls(by_url, resolved_tag, docs_root=docs_root, public_assets=public_assets)
    return list(by_url.values())


def add_docs_linked_urls(
    by_url: dict[str, FreshnessUrl],
    resolved_tag: str,
    *,
    docs_root: Path,
    public_assets: dict[str, FreshnessAsset],
) -> None:
    install_asset = public_assets["install.sh"]
    for snippet in public_command_inventory(docs_root):
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
        sources=(*current.sources, source),
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


def public_url_curl_args(curl_bin: str, url: str) -> list[str]:
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
        "--output",
        "/dev/null",
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
        return ""
    if any(f"failure={failure}" in message for failure in NETWORK_FAILURE_KINDS):
        return "network-transient"
    if "failure=latest_tag_switch" in message or "latest tag" in message:
        return "stale-latest"
    if "freshness public asset missing" in message:
        return "missing-public-asset"
    if "checked_command_inventory_digest" in message:
        return "docs-drift"
    if "digest mismatch" in message or "checksum" in message:
        return "checksum-mismatch"
    if "URL mismatch" in message or "size mismatch" in message:
        return "provenance-mismatch"
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


def freshness_proof_json(
    resolution,
    checks: list[FreshnessUrl],
    public_assets: dict[str, FreshnessAsset],
) -> dict:
    return {
        "schema_version": 1,
        "freshness_network_bounded": True,
        "repository": resolution.repository,
        "resolved_tag": resolution.resolved_tag,
        "published_at": utc_now_rfc3339(),
        "fetch_policy": {
            "connect_timeout_seconds": FETCH_CONNECT_TIMEOUT_SECONDS,
            "max_time_seconds": FETCH_MAX_TIME_SECONDS,
            "retry_count": FETCH_RETRY_COUNT,
            "retry_delay_seconds": FETCH_RETRY_DELAY_SECONDS,
        },
        "checked_urls": [
            {
                "role": check.role,
                "url": check.url,
                "release_tag": check.release_tag,
                "asset_name": check.asset_name,
                "sources": list(check.sources),
                "size_bytes": check.size_bytes,
                "sha256": check.sha256,
            }
            for check in checks
        ],
        "public_assets": public_asset_proof_rows(public_assets),
    }


def utc_now_rfc3339() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def public_asset_proof_rows(public_assets: dict[str, FreshnessAsset]) -> list[dict]:
    return [
        {
            "name": asset.name,
            "role": asset.role,
            "url": asset.url,
            "release_tag": asset.release_tag,
            "size_bytes": asset.size_bytes,
            "sha256": asset.sha256,
        }
        for _name, asset in sorted(public_assets.items())
    ]


if __name__ == "__main__":
    raise SystemExit(main())
