#!/usr/bin/env python3
"""Resolve GitHub latest to one stable m80 release and emit pinned install inputs."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import subprocess

from release_url_contract import pinned_install_command, public_release_root, release_asset_url
from stable_release_channel import (
    REQUIRED_PUBLIC_ASSETS,
    read_json,
    validate_stable_release_metadata,
)


@dataclass(frozen=True)
class MetadataSource:
    release: dict
    label: str
    mode: str


@dataclass(frozen=True)
class LatestBootstrapResolution:
    repository: str
    resolved_tag: str
    latest_source_mode: str
    guard_source_mode: str
    pinned_asset_urls: dict[str, str]

    @property
    def install_url(self) -> str:
        return self.pinned_asset_urls["install.sh"]

    @property
    def pinned_install_command(self) -> str:
        return pinned_install_command(self.resolved_tag)


def parse_args() -> argparse.Namespace:
    root = public_release_root()
    parser = argparse.ArgumentParser(description=__doc__)
    latest = parser.add_mutually_exclusive_group(required=True)
    latest.add_argument(
        "--latest-url",
        help="GitHub release metadata URL for the latest release, for example "
        f"https://api.github.com/repos/{root.repository}/releases/latest",
    )
    latest.add_argument("--latest-metadata", type=Path, help="local latest release metadata JSON fixture")
    guard = parser.add_mutually_exclusive_group()
    guard.add_argument("--guard-latest-url", help="second latest release metadata URL used for the tag-switch guard")
    guard.add_argument("--guard-metadata", type=Path, help="second local latest metadata fixture used for the tag-switch guard")
    parser.add_argument("--asset-index", type=Path, help="optional local release asset index fixture to validate")
    parser.add_argument("--curl", default="curl", help="curl binary used for URL metadata fetches")
    parser.add_argument("--json", action="store_true", help="render the pinned bootstrap resolution as JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    asset_index = read_json(args.asset_index, "release asset index") if args.asset_index else None
    try:
        latest = load_latest_source(args.latest_metadata, args.latest_url, curl_bin=args.curl, label="latest release metadata")
        guard = load_guard_source(args, latest)
        resolution = resolve_latest_bootstrap(latest, guard=guard, asset_index=asset_index)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc

    if args.json:
        print(json.dumps(resolution_json(resolution), indent=2, sort_keys=True))
    else:
        print(f"stable latest resolved: repository={resolution.repository} tag={resolution.resolved_tag}")
        print(f"install_url={resolution.install_url}")
        print(f"pinned_command={resolution.pinned_install_command}")
    return 0


def load_latest_source(path: Path | None, url: str | None, *, curl_bin: str, label: str) -> MetadataSource:
    if path is not None:
        return MetadataSource(read_json(path, label), str(path), "fixture")
    if url is not None:
        return MetadataSource(fetch_json_url(url, curl_bin=curl_bin, label=label), url, "url")
    raise ValueError(f"{label} source missing")


def load_guard_source(args: argparse.Namespace, latest: MetadataSource) -> MetadataSource:
    if args.guard_metadata is not None:
        return load_latest_source(args.guard_metadata, None, curl_bin=args.curl, label="guard latest release metadata")
    if args.guard_latest_url is not None:
        return load_latest_source(None, args.guard_latest_url, curl_bin=args.curl, label="guard latest release metadata")
    if args.latest_url is not None:
        return load_latest_source(None, args.latest_url, curl_bin=args.curl, label="guard latest release metadata")
    return MetadataSource(latest.release, latest.label, "fixture-reused")


def resolve_latest_bootstrap(
    latest: MetadataSource,
    *,
    guard: MetadataSource,
    asset_index: dict | None = None,
) -> LatestBootstrapResolution:
    eligibility = validate_stable_release_metadata(latest.release, asset_index=asset_index)
    guard_tag = release_tag_from_metadata(guard.release, "guard latest release metadata")
    require_same_latest_tag(eligibility.tag, guard_tag)
    validate_stable_release_metadata(guard.release, expected_tag=eligibility.tag)
    pinned_asset_urls = {name: release_asset_url(eligibility.tag, name) for name in REQUIRED_PUBLIC_ASSETS}
    return LatestBootstrapResolution(
        repository=eligibility.repository,
        resolved_tag=eligibility.tag,
        latest_source_mode=latest.mode,
        guard_source_mode=guard.mode,
        pinned_asset_urls=pinned_asset_urls,
    )


def release_tag_from_metadata(release: dict, label: str) -> str:
    if not isinstance(release, dict):
        raise ValueError(f"{label} must be a JSON object")
    tag = release.get("tag_name")
    if not isinstance(tag, str) or not tag:
        raise ValueError(f"{label} tag_name must be a nonempty string")
    return tag


def require_same_latest_tag(first_tag: str, second_tag: str) -> None:
    if first_tag == second_tag:
        return
    raise ValueError(
        "latest release tag changed during bootstrap resolution: "
        f"started with {first_tag}, guard observed {second_tag}; "
        f"retry with pinned command: {pinned_install_command(first_tag)}"
    )


def fetch_json_url(url: str, *, curl_bin: str, label: str) -> dict:
    result = subprocess.run(
        [curl_bin, "-fsSL", url],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no curl output"
        raise ValueError(f"failed to fetch {label} from {url}: curl exited {result.returncode}: {detail}")
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ValueError(f"{label} from {url} must be valid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"{label} from {url} must be a JSON object")
    return value


def resolution_json(resolution: LatestBootstrapResolution) -> dict:
    return {
        "stable_latest_resolved": True,
        "repository": resolution.repository,
        "resolved_tag": resolution.resolved_tag,
        "latest_source_mode": resolution.latest_source_mode,
        "tag_switch_guard_source_mode": resolution.guard_source_mode,
        "install_url": resolution.install_url,
        "pinned_install_command": resolution.pinned_install_command,
        "pinned_asset_urls": resolution.pinned_asset_urls,
    }


if __name__ == "__main__":
    raise SystemExit(main())
