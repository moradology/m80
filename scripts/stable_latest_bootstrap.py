#!/usr/bin/env python3
"""Resolve GitHub latest to one stable m80 release and emit pinned install inputs."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import shlex
import subprocess

from release_url_contract import pinned_install_command, public_release_root, release_asset_url
from stable_release_channel import (
    REQUIRED_PUBLIC_ASSETS,
    public_asset_role,
    read_json,
    validate_stable_release_metadata,
)


METADATA_CONNECT_TIMEOUT_SECONDS = 10
METADATA_MAX_TIME_SECONDS = 120
METADATA_RETRY_COUNT = 2
METADATA_RETRY_DELAY_SECONDS = 1


@dataclass(frozen=True)
class MetadataSource:
    release: dict
    label: str
    mode: str
    role: str = "latest release metadata"
    fetch_role: str = "initial"


@dataclass(frozen=True)
class LatestBootstrapResolution:
    repository: str
    resolved_tag: str
    latest_source_mode: str
    guard_source_mode: str
    pinned_asset_urls: dict[str, str]
    install_root: str | None = None
    asset_index_path: str | None = None

    @property
    def install_url(self) -> str:
        return self.pinned_asset_urls["install.sh"]

    @property
    def pinned_install_command(self) -> str:
        return pinned_install_command(self.resolved_tag)

    @property
    def versioned_install_args(self) -> list[str]:
        args = ["install", "--bootstrap-tag", self.resolved_tag]
        if self.install_root is not None:
            args.extend(["--install-root", self.install_root])
        return args

    @property
    def versioned_install_inputs(self) -> dict:
        asset_index = {"url": self.pinned_asset_urls["m80-release-assets.json"]}
        if self.asset_index_path is not None:
            asset_index["path"] = self.asset_index_path
        return {
            "release_tag": self.resolved_tag,
            "install_root": self.install_root,
            "asset_index": asset_index,
            "asset_urls": self.pinned_asset_urls,
            "checksum_urls": urls_for_roles(self.pinned_asset_urls, {"checksum"}),
            "proof_urls": urls_for_roles(self.pinned_asset_urls, {"provenance", "attestation"}),
        }


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
    parser.add_argument("--install-root", help="optional install root argument to pass to versioned install logic")
    parser.add_argument("--curl", default="curl", help="curl binary used for URL metadata fetches")
    parser.add_argument("--json", action="store_true", help="render the pinned bootstrap resolution as JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    asset_index = read_json(args.asset_index, "release asset index") if args.asset_index else None
    try:
        latest = load_latest_source(args.latest_metadata, args.latest_url, curl_bin=args.curl, label="latest release metadata")
        guard = load_guard_source(args, latest)
        resolution = resolve_latest_bootstrap(
            latest,
            guard=guard,
            asset_index=asset_index,
            install_root=args.install_root,
            asset_index_path=str(args.asset_index) if args.asset_index else None,
        )
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc

    if args.json:
        print(json.dumps(resolution_json(resolution), indent=2, sort_keys=True))
    else:
        print(f"stable latest resolved: repository={resolution.repository} tag={resolution.resolved_tag}")
        print(f"install_url={resolution.install_url}")
        print(f"pinned_command={resolution.pinned_install_command}")
        print(f"versioned_install_args={shlex.join(resolution.versioned_install_args)}")
    return 0


def load_latest_source(path: Path | None, url: str | None, *, curl_bin: str, label: str) -> MetadataSource:
    fetch_role = metadata_fetch_role(label)
    if path is not None:
        return MetadataSource(read_json(path, label), str(path), "fixture", label, fetch_role)
    if url is not None:
        return MetadataSource(fetch_json_url(url, curl_bin=curl_bin, label=label), url, "url", label, fetch_role)
    raise ValueError(f"{label} source missing")


def load_guard_source(args: argparse.Namespace, latest: MetadataSource) -> MetadataSource:
    if args.guard_metadata is not None:
        return load_latest_source(args.guard_metadata, None, curl_bin=args.curl, label="guard latest release metadata")
    if args.guard_latest_url is not None:
        return load_latest_source(None, args.guard_latest_url, curl_bin=args.curl, label="guard latest release metadata")
    if args.latest_url is not None:
        return load_latest_source(None, args.latest_url, curl_bin=args.curl, label="guard latest release metadata")
    return MetadataSource(latest.release, latest.label, "fixture-reused", "guard latest release metadata", "guard")


def resolve_latest_bootstrap(
    latest: MetadataSource,
    *,
    guard: MetadataSource,
    asset_index: dict | None = None,
    install_root: str | None = None,
    asset_index_path: str | None = None,
) -> LatestBootstrapResolution:
    eligibility = validate_stable_release_metadata(latest.release, asset_index=asset_index)
    guard_tag = release_tag_from_metadata(guard.release, describe_metadata_source(guard))
    require_same_latest_tag(eligibility.tag, guard_tag, latest=latest, guard=guard)
    validate_stable_release_metadata(guard.release, expected_tag=eligibility.tag)
    pinned_asset_urls = {name: release_asset_url(eligibility.tag, name) for name in REQUIRED_PUBLIC_ASSETS}
    return LatestBootstrapResolution(
        repository=eligibility.repository,
        resolved_tag=eligibility.tag,
        latest_source_mode=latest.mode,
        guard_source_mode=guard.mode,
        pinned_asset_urls=pinned_asset_urls,
        install_root=install_root,
        asset_index_path=asset_index_path,
    )


def release_tag_from_metadata(release: dict, label: str) -> str:
    if not isinstance(release, dict):
        raise ValueError(f"{label} must be a JSON object")
    tag = release.get("tag_name")
    if not isinstance(tag, str) or not tag:
        raise ValueError(f"{label} tag_name must be a nonempty string")
    return tag


def require_same_latest_tag(first_tag: str, second_tag: str, *, latest: MetadataSource, guard: MetadataSource) -> None:
    if first_tag == second_tag:
        return
    raise ValueError(
        "latest release tag changed during bootstrap resolution: "
        f"started with {first_tag}, guard observed {second_tag}; "
        f"failure=latest_tag_switch; "
        f"initial_source={describe_metadata_source(latest)}; "
        f"guard_source={describe_metadata_source(guard)}; "
        f"retry with pinned command: {pinned_install_command(first_tag)}"
    )


def describe_metadata_source(source: MetadataSource) -> str:
    return f"{source.role} from {source.label} fetch_role={source.fetch_role}"


def metadata_fetch_role(label: str) -> str:
    if label.startswith("guard "):
        return "guard"
    return "initial"


def metadata_curl_args(curl_bin: str, url: str) -> list[str]:
    return [
        curl_bin,
        "-fsSL",
        "--connect-timeout",
        str(METADATA_CONNECT_TIMEOUT_SECONDS),
        "--max-time",
        str(METADATA_MAX_TIME_SECONDS),
        "--retry",
        str(METADATA_RETRY_COUNT),
        "--retry-delay",
        str(METADATA_RETRY_DELAY_SECONDS),
        url,
    ]


def curl_failure_kind(status: int) -> str:
    if status in {6, 7}:
        return "dns_or_connect_failure"
    if status == 22:
        return "http_failure"
    if status == 28:
        return "timeout"
    if status == 130:
        return "interrupted"
    return "metadata_fetch_failure"


def fetch_json_url(url: str, *, curl_bin: str, label: str) -> dict:
    fetch_role = metadata_fetch_role(label)
    try:
        result = subprocess.run(
            metadata_curl_args(curl_bin, url),
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise ValueError(
            f"failed to fetch {label} from {url}: fetch_role={fetch_role} failure=curl_spawn_failed: {exc}"
        ) from exc
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no curl output"
        raise ValueError(
            f"failed to fetch {label} from {url}: "
            f"fetch_role={fetch_role} failure={curl_failure_kind(result.returncode)} curl_exit={result.returncode}; "
            f"curl exited {result.returncode}: {detail}"
        )
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ValueError(
            f"failed to parse {label} from {url}: fetch_role={fetch_role} failure=malformed_json: {exc}"
        ) from exc
    if not isinstance(value, dict):
        raise ValueError(
            f"failed to parse {label} from {url}: fetch_role={fetch_role} "
            "failure=malformed_json: expected JSON object"
        )
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
        "versioned_install_args": resolution.versioned_install_args,
        "versioned_install_inputs": resolution.versioned_install_inputs,
        "bootstrap_proof": {
            "resolved_tag": resolution.resolved_tag,
            "pinned_asset_urls": resolution.pinned_asset_urls,
        },
    }


def urls_for_roles(asset_urls: dict[str, str], roles: set[str]) -> dict[str, str]:
    return {name: url for name, url in asset_urls.items() if public_asset_role(name) in roles}


if __name__ == "__main__":
    raise SystemExit(main())
