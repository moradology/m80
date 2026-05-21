#!/usr/bin/env python3
"""Bounded public release freshness checks for the m80 install surface."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import subprocess

from quickstart_snippets import INSTALL_URL_RE, public_command_inventory
from release_url_contract import public_release_root
from stable_latest_bootstrap import (
    METADATA_CONNECT_TIMEOUT_SECONDS,
    METADATA_MAX_TIME_SECONDS,
    METADATA_RETRY_COUNT,
    METADATA_RETRY_DELAY_SECONDS,
    load_latest_source,
    resolve_latest_bootstrap,
)
from stable_release_channel import read_json


FETCH_CONNECT_TIMEOUT_SECONDS = METADATA_CONNECT_TIMEOUT_SECONDS
FETCH_MAX_TIME_SECONDS = METADATA_MAX_TIME_SECONDS
FETCH_RETRY_COUNT = METADATA_RETRY_COUNT
FETCH_RETRY_DELAY_SECONDS = METADATA_RETRY_DELAY_SECONDS


@dataclass(frozen=True)
class FreshnessUrl:
    role: str
    url: str
    asset_name: str
    release_tag: str
    sources: tuple[str, ...]


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
        guard = load_freshness_guard_source(args, latest)
        resolution = resolve_latest_bootstrap(latest, guard=guard, asset_index=asset_index)
        checks = freshness_urls(resolution.resolved_tag, resolution.pinned_asset_urls, docs_root=args.docs_root)
        for check in checks:
            fetch_public_url(check, curl_bin=args.curl)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc

    proof = freshness_proof_json(resolution, checks)
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


def freshness_urls(resolved_tag: str, pinned_asset_urls: dict[str, str], *, docs_root: Path) -> list[FreshnessUrl]:
    release_root = public_release_root()
    by_url: dict[str, FreshnessUrl] = {}
    add_freshness_url(
        by_url,
        role="latest-install",
        url=release_root.latest_install_url,
        asset_name="install.sh",
        release_tag="latest",
        source="release-url-contract:latest-install",
    )
    add_freshness_url(
        by_url,
        role="pinned-install",
        url=release_root.pinned_install_url(resolved_tag),
        asset_name="install.sh",
        release_tag=resolved_tag,
        source="release-url-contract:pinned-install",
    )
    for asset_name, url in sorted(pinned_asset_urls.items()):
        add_freshness_url(
            by_url,
            role="public-asset",
            url=url,
            asset_name=asset_name,
            release_tag=resolved_tag,
            source="stable-latest-bootstrap:pinned-assets",
        )
    add_docs_linked_urls(by_url, resolved_tag, docs_root=docs_root)
    return list(by_url.values())


def add_docs_linked_urls(by_url: dict[str, FreshnessUrl], resolved_tag: str, *, docs_root: Path) -> None:
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
) -> None:
    current = by_url.get(url)
    if current is None:
        by_url[url] = FreshnessUrl(
            role=role,
            url=url,
            asset_name=asset_name,
            release_tag=release_tag,
            sources=(source,),
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
    )


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
    fields = [
        "freshness public URL fetch failed",
        f"failure={failure}",
        f"url={check.url}",
        f"release_tag={check.release_tag}",
        f"asset={check.asset_name}",
        f"role={check.role}",
        f"sources={','.join(check.sources)}",
    ]
    if curl_exit is not None:
        fields.append(f"curl_exit={curl_exit}")
    fields.append(detail)
    return "; ".join(fields)


def freshness_proof_json(resolution, checks: list[FreshnessUrl]) -> dict:
    return {
        "freshness_network_bounded": True,
        "repository": resolution.repository,
        "resolved_tag": resolution.resolved_tag,
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
            }
            for check in checks
        ],
    }


if __name__ == "__main__":
    raise SystemExit(main())
