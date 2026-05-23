#!/usr/bin/env python3
"""Build and verify the public no-auth release readiness receipt."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import urllib.error
import urllib.request


SCHEMA_VERSION = 1
KIND = "m80_release_readiness_public_access"
LANE_ID = "public-access-latest"
PROOF_KIND = "public-access-proof"
SUBSTRATE_KIND = "public-github"
REQUIRED_PUBLIC_ASSETS = {
    "SHA256SUMS",
    "install.sh",
    "install.sh.sha256",
    "m80-bootstrap-selector.tsv",
    "m80-bootstrap-selector.tsv.sha256",
    "m80-linux-x86_64.bundle.json",
    "m80-linux-x86_64.bundle.json.sha256",
    "m80-linux-x86_64.tar.gz",
    "m80-linux-x86_64.tar.gz.sha256",
    "m80-release-assets.json",
    "m80-release-assets.json.sha256",
    "m80-release-attestation.json",
    "m80-release-build.json",
    "m80-release-build.json.sha256",
    "m80-release-integrity.attestation.jsonl",
    "m80-release-integrity.json",
}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class VerificationError(ValueError):
    pass


@dataclass(frozen=True)
class FetchResult:
    url: str
    final_url: str
    http_status: int
    body: bytes
    error: str | None = None
    redirects: tuple[str, ...] = ()


@dataclass(frozen=True)
class LatestState:
    api: dict
    install: FetchResult


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="moradology/m80")
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--verification-time")
    parser.add_argument("--latest-timeout-seconds", type=float, default=0.0)
    parser.add_argument("--latest-poll-interval-seconds", type=float, default=2.0)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.write:
            receipt = build_receipt(
                repository=args.repository,
                release_tag=args.release_tag,
                commit_sha=args.commit_sha,
                verification_time=args.verification_time,
                latest_timeout_seconds=args.latest_timeout_seconds,
                latest_poll_interval_seconds=args.latest_poll_interval_seconds,
            )
            write_json(args.out, receipt)
        receipt = read_json(args.out, "public-access release readiness receipt")
        verify_receipt(
            receipt,
            expected_repository=args.repository,
            expected_release_tag=args.release_tag,
            expected_commit_sha=args.commit_sha,
        )
    except VerificationError as exc:
        raise SystemExit(str(exc)) from exc
    print(f"public-access release readiness receipt ok: {args.out}")
    return 0


def build_receipt(
    *,
    repository: str,
    release_tag: str,
    commit_sha: str,
    verification_time: str | None = None,
    latest_timeout_seconds: float = 0.0,
    latest_poll_interval_seconds: float = 2.0,
) -> dict:
    require_safe_repository(repository)
    check(not os.environ.get("GH_TOKEN"), "GH_TOKEN must be unset for public-access proof")
    check(not os.environ.get("GITHUB_TOKEN"), "GITHUB_TOKEN must be unset for public-access proof")

    api_root = f"https://api.github.com/repos/{repository}/releases"
    release_url = f"https://github.com/{repository}/releases/tag/{release_tag}"
    latest_install_url = f"https://github.com/{repository}/releases/latest/download/install.sh"
    pinned_install_url = f"https://github.com/{repository}/releases/download/{release_tag}/install.sh"

    latest_state = wait_for_public_latest_state(
        api_root=api_root,
        repository=repository,
        release_tag=release_tag,
        release_url=release_url,
        latest_install_url=latest_install_url,
        timeout_seconds=latest_timeout_seconds,
        poll_interval_seconds=latest_poll_interval_seconds,
    )
    latest_api = latest_state.api
    latest_install = latest_state.install
    pinned_api = fetch_json(f"{api_root}/tags/{release_tag}")
    check_release_payload(pinned_api["json"], repository, release_tag, release_url, "pinned release")
    release_assets = assets_by_name(pinned_api["json"], repository, release_tag)
    check_asset_set(set(release_assets), REQUIRED_PUBLIC_ASSETS, "public release asset set")

    pinned_install = fetch_bytes(pinned_install_url)
    resolved_latest_tag = tag_from_download_result(latest_install)
    check(
        sha256_bytes(latest_install.body) == sha256_bytes(pinned_install.body),
        "latest/pinned install.sh digest disagreement",
    )

    asset_downloads = []
    by_name_body: dict[str, bytes] = {}
    for name in sorted(REQUIRED_PUBLIC_ASSETS):
        result = fetch_bytes(release_assets[name]["browser_download_url"])
        by_name_body[name] = result.body
        asset_downloads.append(download_row(name, result))

    integrity = parse_downloaded_json(by_name_body, "m80-release-integrity.json")
    build = parse_downloaded_json(by_name_body, "m80-release-build.json")
    asset_index = parse_downloaded_json(by_name_body, "m80-release-assets.json")
    check(integrity.get("repository") == repository, "release integrity repository mismatch")
    check(integrity.get("release_tag") == release_tag, "release integrity release_tag mismatch")
    check(integrity.get("commit_sha") == commit_sha, "release integrity commit_sha mismatch")
    check(build.get("source_commit") == commit_sha, "release build source_commit mismatch")
    check(build.get("release_tag") == release_tag, "release build release_tag mismatch")
    check(asset_index.get("release_tag") == release_tag, "asset index release_tag mismatch")

    expected_subjects = integrity_subjects_by_name(integrity)
    for row in asset_downloads:
        subject = expected_subjects.get(row["name"])
        if subject is None:
            continue
        row["expected_sha256"] = subject["sha256"]
        row["expected_size_bytes"] = subject["size_bytes"]
        check(row["sha256"] == subject["sha256"], f"downloaded asset {row['name']} sha256 mismatch")
        check(row["size_bytes"] == subject["size_bytes"], f"downloaded asset {row['name']} size mismatch")

    asset_manifest = next(row for row in asset_downloads if row["name"] == "m80-release-assets.json")
    receipt = {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "lane_id": LANE_ID,
        "proof_kind": PROOF_KIND,
        "status": "passed",
        "repository": repository,
        "release_tag": release_tag,
        "commit_sha": commit_sha,
        "github_release_url": release_url,
        "verification_time": verification_time or utc_now(),
        "substrate": {
            "kind": SUBSTRATE_KIND,
            "fixture": False,
        },
        "auth": {
            "GH_TOKEN": False,
            "GITHUB_TOKEN": False,
            "gh_auth_present": gh_auth_present(),
            "authorization_header_used": False,
        },
        "latest_install_url": latest_install_url,
        "pinned_install_url": pinned_install_url,
        "resolved_latest_tag": resolved_latest_tag,
        "expected_stable_tag": release_tag,
        "latest_pinned_agree": True,
        "api_checks": {
            "latest_release": api_check_row(f"{api_root}/latest", latest_api["fetch"]),
            "pinned_release": api_check_row(f"{api_root}/tags/{release_tag}", pinned_api["fetch"]),
        },
        "installer_downloads": {
            "latest_install": download_row("install.sh", latest_install),
            "pinned_install": download_row("install.sh", pinned_install),
        },
        "asset_manifest": {
            "name": "m80-release-assets.json",
            "sha256": asset_manifest["sha256"],
            "size_bytes": asset_manifest["size_bytes"],
        },
        "downloaded_assets": asset_downloads,
        "downloaded_asset_digests": {row["name"]: row["sha256"] for row in asset_downloads},
        "integrity_subjects": sorted(expected_subjects.values(), key=lambda row: row["name"]),
        "release_build": {
            "source_commit": build["source_commit"],
            "release_tag": build["release_tag"],
            "builder_identity": build.get("builder_identity"),
        },
        "command": "GH_CONFIG_DIR=/tmp/m80-noauth-gh env -u GH_TOKEN -u GITHUB_TOKEN "
        "scripts/release_public_access_receipt.py --repository "
        f"{repository} --release-tag {release_tag} --commit-sha {commit_sha} "
        "--out docs/behaviors/release/release-readiness-public-access.json --write",
        "exit_status": 0,
        "stdout": "public-access release readiness receipt ok",
        "stderr": "",
    }
    check(not receipt["auth"]["gh_auth_present"], "GitHub CLI auth must be absent for public-access proof")
    verify_receipt(
        receipt,
        expected_repository=repository,
        expected_release_tag=release_tag,
        expected_commit_sha=commit_sha,
    )
    return receipt


def wait_for_public_latest_state(
    *,
    api_root: str,
    repository: str,
    release_tag: str,
    release_url: str,
    latest_install_url: str,
    timeout_seconds: float,
    poll_interval_seconds: float,
) -> LatestState:
    deadline = time.monotonic() + max(timeout_seconds, 0.0)
    while True:
        try:
            latest_api = fetch_json(f"{api_root}/latest")
            check_release_payload(latest_api["json"], repository, release_tag, release_url, "latest release")
            latest_install = fetch_bytes(latest_install_url)
            resolved_latest_tag = tag_from_download_result(latest_install)
            check(resolved_latest_tag == release_tag, "latest install URL resolved to stale tag")
            return LatestState(api=latest_api, install=latest_install)
        except VerificationError as exc:
            if not is_transient_latest_mismatch(str(exc)) or time.monotonic() >= deadline:
                raise
            time.sleep(max(poll_interval_seconds, 0.0))


def is_transient_latest_mismatch(message: str) -> bool:
    return message in {
        "latest release tag_name mismatch",
        "latest release html_url mismatch",
        "latest release asset download URL has wrong owner/repo or tag",
        "latest install URL resolved to stale tag",
    }


def verify_receipt(
    receipt: dict,
    *,
    expected_repository: str,
    expected_release_tag: str,
    expected_commit_sha: str,
) -> None:
    require_fields(
        receipt,
        {
            "schema_version",
            "kind",
            "lane_id",
            "proof_kind",
            "status",
            "repository",
            "release_tag",
            "commit_sha",
            "github_release_url",
            "verification_time",
            "substrate",
            "auth",
            "latest_install_url",
            "pinned_install_url",
            "resolved_latest_tag",
            "expected_stable_tag",
            "latest_pinned_agree",
            "api_checks",
            "installer_downloads",
            "asset_manifest",
            "downloaded_assets",
            "downloaded_asset_digests",
            "integrity_subjects",
            "release_build",
            "command",
            "exit_status",
            "stdout",
            "stderr",
        },
        "public-access receipt",
    )
    check(receipt["schema_version"] == SCHEMA_VERSION, "unsupported public-access receipt schema_version")
    check(receipt["kind"] == KIND, "public-access receipt kind mismatch")
    check(receipt["lane_id"] == LANE_ID, "public-access receipt lane_id mismatch")
    check(receipt["proof_kind"] == PROOF_KIND, "public-access receipt proof_kind mismatch")
    check(receipt["status"] == "passed", "public-access receipt status must be passed")
    check(receipt["repository"] == expected_repository, "wrong owner/repo in public-access receipt")
    check(receipt["release_tag"] == expected_release_tag, "stale release tag in public-access receipt")
    check(receipt["commit_sha"] == expected_commit_sha, "stale commit in public-access receipt")
    check(
        receipt["github_release_url"] == f"https://github.com/{expected_repository}/releases/tag/{expected_release_tag}",
        "GitHub release URL mismatch",
    )
    check(parse_timestamp(receipt["verification_time"]) is not None, "verification_time invalid")

    substrate = require_dict(receipt["substrate"], "public-access receipt substrate")
    check(substrate.get("kind") == SUBSTRATE_KIND, "public-access substrate kind mismatch")
    check(substrate.get("fixture") is False, "fixture receipts cannot satisfy the real public-access lane")

    auth = require_dict(receipt["auth"], "public-access receipt auth")
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "gh_auth_present", "authorization_header_used"]:
        check(auth.get(key) is False, f"public-access proof used auth: {key}")

    check(receipt["expected_stable_tag"] == expected_release_tag, "expected stable tag mismatch")
    check(receipt["resolved_latest_tag"] == expected_release_tag, "latest install URL resolved to stale tag")
    check(receipt["latest_pinned_agree"] is True, "latest/pinned tag disagreement")
    check(
        receipt["latest_install_url"] == f"https://github.com/{expected_repository}/releases/latest/download/install.sh",
        "latest install URL mismatch",
    )
    check(
        receipt["pinned_install_url"]
        == f"https://github.com/{expected_repository}/releases/download/{expected_release_tag}/install.sh",
        "pinned install URL mismatch",
    )

    api_checks = require_dict(receipt["api_checks"], "public-access receipt api_checks")
    for name in ["latest_release", "pinned_release"]:
        verify_http_row(require_dict(api_checks.get(name), f"api check {name}"), expected_repository, expected_release_tag)

    installer_downloads = require_dict(receipt["installer_downloads"], "public-access receipt installer_downloads")
    latest_install = require_dict(installer_downloads.get("latest_install"), "latest install download")
    pinned_install = require_dict(installer_downloads.get("pinned_install"), "pinned install download")
    verify_download_row(latest_install, expected_repository, expected_release_tag)
    verify_download_row(pinned_install, expected_repository, expected_release_tag)
    check(latest_install["sha256"] == pinned_install["sha256"], "latest/pinned install.sh digest disagreement")

    downloaded_assets = require_list(receipt["downloaded_assets"], "public-access receipt downloaded_assets")
    by_name = {}
    for row in downloaded_assets:
        asset = require_dict(row, "public-access receipt downloaded asset")
        name = require_asset_name(asset.get("name"), "downloaded asset name")
        check(name not in by_name, f"duplicate downloaded asset: {name}")
        verify_download_row(asset, expected_repository, expected_release_tag)
        by_name[name] = asset
    check_asset_set(set(by_name), REQUIRED_PUBLIC_ASSETS, "public-access downloaded asset set")

    digests = require_dict(receipt["downloaded_asset_digests"], "downloaded_asset_digests")
    check_asset_set(set(digests), REQUIRED_PUBLIC_ASSETS, "downloaded_asset_digests set")
    for name, digest in digests.items():
        require_sha256(digest, f"downloaded_asset_digests {name}")
        check(digest == by_name[name]["sha256"], f"downloaded_asset_digests mismatch for {name}")

    asset_manifest = require_dict(receipt["asset_manifest"], "asset_manifest")
    check(asset_manifest.get("name") == "m80-release-assets.json", "asset manifest name mismatch")
    check(asset_manifest.get("sha256") == by_name["m80-release-assets.json"]["sha256"], "asset manifest digest mismatch")
    check(
        asset_manifest.get("size_bytes") == by_name["m80-release-assets.json"]["size_bytes"],
        "asset manifest size mismatch",
    )

    subjects = require_list(receipt["integrity_subjects"], "integrity_subjects")
    subject_by_name = {}
    for row in subjects:
        subject = require_dict(row, "integrity subject")
        name = require_asset_name(subject.get("name"), "integrity subject name")
        check(name not in subject_by_name, f"duplicate integrity subject: {name}")
        require_sha256(subject.get("sha256"), f"integrity subject {name} sha256")
        require_non_negative_int(subject.get("size_bytes"), f"integrity subject {name} size_bytes")
        subject_by_name[name] = subject
    check("install.sh" in subject_by_name, "integrity subjects missing install.sh")
    for name, subject in subject_by_name.items():
        check(name in by_name, f"integrity subject not downloaded: {name}")
        check(by_name[name]["sha256"] == subject["sha256"], f"downloaded asset {name} sha256 mismatch")
        check(by_name[name]["size_bytes"] == subject["size_bytes"], f"downloaded asset {name} size mismatch")

    release_build = require_dict(receipt["release_build"], "release_build")
    check(release_build.get("source_commit") == expected_commit_sha, "release_build source_commit mismatch")
    check(release_build.get("release_tag") == expected_release_tag, "release_build release_tag mismatch")
    check(isinstance(receipt["command"], str) and receipt["command"], "public-access receipt command missing")
    check(receipt["exit_status"] == 0, "public-access receipt exit_status must be 0")
    check(isinstance(receipt["stdout"], str), "public-access receipt stdout must be a string")
    check(isinstance(receipt["stderr"], str), "public-access receipt stderr must be a string")


def fetch_json(url: str) -> dict:
    result = fetch_bytes(url, accept="application/vnd.github+json")
    try:
        payload = json.loads(result.body)
    except json.JSONDecodeError as exc:
        raise VerificationError(f"GitHub API response was not JSON: {url}: {exc}") from exc
    check(isinstance(payload, dict), f"GitHub API response must be an object: {url}")
    return {"fetch": result, "json": payload}


def fetch_bytes(url: str, *, accept: str = "application/octet-stream") -> FetchResult:
    redirects: list[str] = []

    class RedirectRecorder(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[no-untyped-def]
            redirects.append(newurl)
            return super().redirect_request(req, fp, code, msg, headers, newurl)

    request = urllib.request.Request(
        url,
        headers={
            "Accept": accept,
            "User-Agent": "m80-public-access-receipt",
        },
    )
    opener = urllib.request.build_opener(RedirectRecorder)
    try:
        with opener.open(request, timeout=60) as response:
            return FetchResult(
                url=url,
                final_url=response.geturl(),
                http_status=response.status,
                body=response.read(),
                redirects=tuple(redirects),
            )
    except urllib.error.HTTPError as exc:
        body = exc.read()
        return FetchResult(
            url=url,
            final_url=exc.geturl(),
            http_status=exc.code,
            body=body,
            error=str(exc),
            redirects=tuple(redirects),
        )
    except urllib.error.URLError as exc:
        raise VerificationError(f"unreachable GitHub API/download: {url}: {exc.reason}") from exc


def check_release_payload(payload: dict, repository: str, release_tag: str, release_url: str, label: str) -> None:
    check(payload.get("tag_name") == release_tag, f"{label} tag_name mismatch")
    check(payload.get("html_url") == release_url, f"{label} html_url mismatch")
    check(payload.get("draft") is False, f"{label} must not be draft")
    check(payload.get("prerelease") is False, f"{label} must not be prerelease")
    check(isinstance(payload.get("assets"), list), f"{label} assets must be a list")
    for asset in payload["assets"]:
        check(isinstance(asset, dict), f"{label} asset must be an object")
        url = asset.get("browser_download_url")
        check(
            isinstance(url, str) and url.startswith(f"https://github.com/{repository}/releases/download/{release_tag}/"),
            f"{label} asset download URL has wrong owner/repo or tag",
        )


def assets_by_name(payload: dict, repository: str, release_tag: str) -> dict[str, dict]:
    by_name = {}
    for asset in payload["assets"]:
        name = require_asset_name(asset.get("name"), "release asset name")
        check(name not in by_name, f"duplicate public release asset: {name}")
        require_non_negative_int(asset.get("size"), f"release asset {name} size")
        check(
            asset.get("browser_download_url") == f"https://github.com/{repository}/releases/download/{release_tag}/{name}",
            f"release asset {name} browser_download_url mismatch",
        )
        by_name[name] = asset
    return by_name


def download_row(name: str, result: FetchResult) -> dict:
    return {
        "name": name,
        "url": result.url,
        "http_status": result.http_status,
        "final_url": result.final_url,
        "size_bytes": len(result.body),
        "sha256": sha256_bytes(result.body),
        "error": result.error,
        "redirects": list(result.redirects),
    }


def api_check_row(url: str, result: FetchResult) -> dict:
    return {
        "url": url,
        "http_status": result.http_status,
        "final_url": result.final_url,
        "error": result.error,
    }


def verify_http_row(row: dict, repository: str, release_tag: str) -> None:
    check(row.get("error") in {None, ""}, "unreachable GitHub API/download")
    status = row.get("http_status")
    check(status not in {401, 403}, "private or auth-required response")
    check(status == 200, f"GitHub API/download HTTP status not 200: {status}")
    final_url = row.get("final_url")
    check(isinstance(final_url, str) and final_url.startswith("https://"), "final URL invalid")
    initial_url = row.get("url")
    if isinstance(initial_url, str) and "/releases/download/" in initial_url:
        check(
            initial_url.startswith(f"https://github.com/{repository}/releases/download/{release_tag}/"),
            "download URL has wrong owner/repo or stale tag",
        )
    if "/releases/download/" in final_url:
        check(
            final_url.startswith(f"https://github.com/{repository}/releases/download/{release_tag}/"),
            "download final URL has wrong owner/repo or stale tag",
        )


def verify_download_row(row: dict, repository: str, release_tag: str) -> None:
    verify_http_row(row, repository, release_tag)
    require_asset_name(row.get("name"), "download row name")
    require_non_negative_int(row.get("size_bytes"), "download row size_bytes")
    require_sha256(row.get("sha256"), "download row sha256")


def integrity_subjects_by_name(integrity: dict) -> dict[str, dict]:
    subjects = integrity.get("subjects")
    check(isinstance(subjects, list), "release integrity subjects must be a list")
    by_name = {}
    for row in subjects:
        check(isinstance(row, dict), "release integrity subject must be an object")
        name = require_asset_name(row.get("name"), "release integrity subject name")
        check(name not in by_name, f"duplicate release integrity subject: {name}")
        require_sha256(row.get("sha256"), f"release integrity subject {name} sha256")
        require_non_negative_int(row.get("size_bytes"), f"release integrity subject {name} size_bytes")
        by_name[name] = {
            "name": name,
            "kind": row.get("kind"),
            "sha256": row["sha256"],
            "size_bytes": row["size_bytes"],
        }
    return by_name


def parse_downloaded_json(by_name_body: dict[str, bytes], name: str) -> dict:
    try:
        payload = json.loads(by_name_body[name])
    except KeyError as exc:
        raise VerificationError(f"downloaded JSON asset missing: {name}") from exc
    except json.JSONDecodeError as exc:
        raise VerificationError(f"downloaded JSON asset invalid: {name}: {exc}") from exc
    check(isinstance(payload, dict), f"downloaded JSON asset must be object: {name}")
    return payload


def tag_from_download_result(result: FetchResult) -> str:
    marker = "/releases/download/"
    for url in [result.url, *result.redirects, result.final_url]:
        if marker in url:
            return url.split(marker, 1)[1].split("/", 1)[0]
    raise VerificationError("download URL does not contain release tag")


def gh_auth_present() -> bool:
    config_dir = Path(os.environ.get("GH_CONFIG_DIR", Path.home() / ".config" / "gh"))
    hosts = config_dir / "hosts.yml"
    if not hosts.exists():
        return False
    try:
        return "oauth_token:" in hosts.read_text()
    except OSError:
        return True


def require_safe_repository(value: str) -> None:
    check(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", value) is not None, "repository must be owner/name")


def require_asset_name(value: object, label: str) -> str:
    check(
        isinstance(value, str)
        and value
        and value not in {".", ".."}
        and "/" not in value
        and re.fullmatch(r"[A-Za-z0-9._+-]+", value) is not None,
        f"{label} must be a flat asset name",
    )
    return value


def require_dict(value: object, label: str) -> dict:
    check(isinstance(value, dict), f"{label} must be an object")
    return value


def require_list(value: object, label: str) -> list:
    check(isinstance(value, list), f"{label} must be a list")
    return value


def require_fields(obj: dict, expected: set[str], label: str) -> None:
    observed = set(obj)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    check(not missing and not extra, f"{label} field mismatch: missing {csv(missing)}; extra {csv(extra)}")


def check_asset_set(observed: set[str], expected: set[str], label: str) -> None:
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    check(not missing and not extra, f"{label} mismatch: missing {csv(missing)}; extra {csv(extra)}")


def require_sha256(value: object, label: str) -> None:
    check(isinstance(value, str) and SHA256_RE.fullmatch(value) is not None, f"{label} must be lowercase sha256")


def require_non_negative_int(value: object, label: str) -> None:
    check(isinstance(value, int) and value >= 0, f"{label} must be a non-negative integer")


def parse_timestamp(value: object) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def read_json(path: Path, label: str) -> dict:
    try:
        payload = json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise VerificationError(f"{label} missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise VerificationError(f"{label} is not valid JSON: {exc}") from exc
    check(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def csv(values: list[str]) -> str:
    return ", ".join(values) if values else "none"


def check(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


if __name__ == "__main__":
    raise SystemExit(main())
