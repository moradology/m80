#!/usr/bin/env python3
"""Validate the public stable release-channel metadata used by m80 installers."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re

from release_url_contract import release_asset_url, release_repository


SCHEMA_VERSION = 2
STABLE_TAG_RE = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+$")
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
INTEGRITY_ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"
REQUIRED_PUBLIC_ASSETS = (
    BUNDLE_NAME,
    METADATA_NAME,
    "install.sh",
    "m80-release-assets.json",
    "m80-bootstrap-selector.tsv",
    "m80-release-build.json",
    "m80-release-integrity.json",
    INTEGRITY_ATTESTATION_BUNDLE_NAME,
    "m80-release-attestation.json",
)


@dataclass(frozen=True)
class StableReleaseEligibility:
    repository: str
    tag: str
    required_assets: tuple[str, ...]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-metadata", required=True, type=Path)
    parser.add_argument("--asset-index", type=Path)
    parser.add_argument("--expected-tag")
    parser.add_argument("--json", action="store_true", help="render the accepted eligibility record as JSON")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    release = read_json(args.release_metadata, "release metadata")
    asset_index = read_json(args.asset_index, "release asset index") if args.asset_index else None
    try:
        eligibility = validate_stable_release_metadata(
            release,
            asset_index=asset_index,
            expected_tag=args.expected_tag,
        )
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    if args.json:
        print(
            json.dumps(
                {
                    "stable_release_eligible": True,
                    "repository": eligibility.repository,
                    "tag": eligibility.tag,
                    "required_assets": list(eligibility.required_assets),
                },
                indent=2,
                sort_keys=True,
            )
        )
    else:
        print(f"stable release eligible: repository={eligibility.repository} tag={eligibility.tag}")
    return 0


def validate_stable_release_metadata(
    release: dict,
    *,
    asset_index: dict | None = None,
    expected_tag: str | None = None,
) -> StableReleaseEligibility:
    require(isinstance(release, dict), "release metadata must be a JSON object")
    tag = require_nonempty_str(release, "tag_name", "release metadata")
    if expected_tag is not None:
        require(
            tag == expected_tag,
            f"release metadata tag mismatch: expected {expected_tag}, got {tag}; {stable_fallback_hint()}",
        )
    require_stable_tag(tag)
    draft = require_bool(release, "draft", "release metadata")
    prerelease = require_bool(release, "prerelease", "release metadata")
    require(not draft, f"stable release ineligible: draft release {tag}; {stable_fallback_hint()}")
    require(not prerelease, f"stable release ineligible: prerelease {tag}; {stable_fallback_hint()}")

    assets = release.get("assets")
    require(isinstance(assets, list), "release metadata assets must be a list")
    by_name = {}
    for asset in assets:
        require(isinstance(asset, dict), "release metadata asset must be an object")
        name = require_nonempty_str(asset, "name", "release metadata asset")
        require(name not in by_name, f"release metadata duplicate asset: {name}")
        by_name[name] = asset

    missing = [name for name in REQUIRED_PUBLIC_ASSETS if name not in by_name]
    require(
        not missing,
        "stable release missing required public asset(s): "
        + "; ".join(missing_public_asset_detail(tag, name) for name in missing)
        + f"; {stable_fallback_hint(tag)}",
    )
    for name in REQUIRED_PUBLIC_ASSETS:
        expected_url = release_asset_url(tag, name)
        actual_url = require_nonempty_str(by_name[name], "browser_download_url", f"release metadata asset {name}")
        require(
            actual_url == expected_url,
            f"release metadata asset URL mismatch for {name}: expected {expected_url}, got {actual_url}",
        )

    if asset_index is not None:
        validate_asset_index(asset_index, tag)

    return StableReleaseEligibility(
        repository=release_repository(),
        tag=tag,
        required_assets=REQUIRED_PUBLIC_ASSETS,
    )


def validate_asset_index(index: dict, tag: str) -> None:
    require(isinstance(index, dict), "release asset index must be a JSON object")
    require(index.get("schema_version") == SCHEMA_VERSION, "release asset index schema_version mismatch")
    require(
        index.get("release_tag") == tag,
        f"release asset index tag mismatch: expected {tag}, got {index.get('release_tag')}",
    )
    assets = index.get("assets")
    require(isinstance(assets, list) and assets, "release asset index assets must be a nonempty list")
    default_rows = []
    for asset in assets:
        require(isinstance(asset, dict), "release asset index asset must be an object")
        name = require_nonempty_str(asset, "name", "release asset index asset")
        asset_tag = require_nonempty_str(asset, "release_tag", f"release asset index asset {name}")
        m80_version = require_nonempty_str(asset, "m80_version", f"release asset index asset {name}")
        require(asset_tag == tag, f"release asset index asset {name} release_tag mismatch: expected {tag}, got {asset_tag}")
        require(m80_version == tag, f"release asset index asset {name} m80_version mismatch: expected {tag}, got {m80_version}")
        if name == BUNDLE_NAME:
            default_rows.append(asset)

    require(len(default_rows) == 1, f"release asset index must contain exactly one default {BUNDLE_NAME} row")
    default = default_rows[0]
    expected_url = release_asset_url(tag, BUNDLE_NAME)
    require(
        default.get("url") == expected_url,
        f"release asset index default bundle URL mismatch: expected {expected_url}, got {default.get('url')}",
    )
    require(default.get("metadata_name") == METADATA_NAME, "release asset index default metadata_name mismatch")
    require(
        default.get("attestation_name") == INTEGRITY_ATTESTATION_BUNDLE_NAME,
        "release asset index default attestation_name mismatch",
    )


def require_stable_tag(tag: str) -> None:
    require(
        STABLE_TAG_RE.fullmatch(tag) is not None,
        f"stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: {tag}",
    )


def stable_fallback_hint(tag: str | None = None) -> str:
    if tag is not None:
        return f"retry pinned command: curl -fsSL {release_asset_url(tag, 'install.sh')} | sudo sh"
    return "select a non-draft, non-prerelease vMAJOR.MINOR.PATCH release and use its pinned install.sh URL"


def missing_public_asset_detail(tag: str, name: str) -> str:
    return f"{name} role={public_asset_role(name)} url={release_asset_url(tag, name)} release_tag={tag}"


def public_asset_role(name: str) -> str:
    if name == "install.sh":
        return "installer"
    if name == "m80-release-assets.json":
        return "asset-index"
    if name == "m80-bootstrap-selector.tsv":
        return "selector"
    if name == "m80-release-integrity.json":
        return "provenance"
    if name == INTEGRITY_ATTESTATION_BUNDLE_NAME or name == "m80-release-attestation.json":
        return "attestation"
    if name == BUNDLE_NAME:
        return "bundle"
    if name == METADATA_NAME or name == "m80-release-build.json":
        return "metadata"
    return "public-asset"


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    with path.open() as f:
        value = json.load(f)
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


def require_nonempty_str(obj: dict, key: str, label: str) -> str:
    value = obj.get(key)
    require(isinstance(value, str) and value, f"{label} {key} must be a nonempty string")
    return value


def require_bool(obj: dict, key: str, label: str) -> bool:
    value = obj.get(key)
    require(isinstance(value, bool), f"{label} {key} must be a boolean")
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


if __name__ == "__main__":
    raise SystemExit(main())
