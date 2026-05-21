#!/usr/bin/env python3
"""Preflight a public latest repair release before any GitHub mutation."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib


SCHEMA_VERSION = 1
MISSING_INSTALLER_LATEST_TAG = "v0.2.6"
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
STABLE_TAG_RE = re.compile(r"^v(?P<major>0|[1-9][0-9]*)\.(?P<minor>0|[1-9][0-9]*)\.(?P<patch>0|[1-9][0-9]*)$")
WORKSPACE_VERSION_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


@dataclass(frozen=True)
class Diagnostic:
    field: str
    expected: str
    observed: str
    safe_repair_action: str

    def to_json(self) -> dict:
        return {
            "field": self.field,
            "expected": self.expected,
            "observed": self.observed,
            "safe_repair_action": self.safe_repair_action,
        }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", required=True, help="candidate stable tag, e.g. v0.2.7")
    parser.add_argument("--source-commit", help="candidate source commit; defaults to git rev-parse HEAD")
    parser.add_argument("--repo-root", default=Path.cwd(), type=Path)
    parser.add_argument("--workspace-version", help="test override for workspace package version")
    parser.add_argument("--tag-commit", help="test/workflow override for the existing tag target; omit when absent")
    parser.add_argument("--latest-metadata", type=Path, help="GitHub latest release metadata JSON")
    parser.add_argument("--existing-latest-tag", help="test override for current public latest tag")
    parser.add_argument("--existing-latest-url", help="test override for current public latest URL")
    parser.add_argument("--missing-installer-latest-tag", default=MISSING_INSTALLER_LATEST_TAG)
    parser.add_argument(
        "--dirty-status",
        choices=("auto", "clean", "dirty"),
        default="auto",
        help="test override for git dirty check; default reads git status",
    )
    parser.add_argument(
        "--dirty-entry",
        action="append",
        default=[],
        help="dirty entry to record when --dirty-status=dirty",
    )
    parser.add_argument("--generated-at", help="ISO-8601 timestamp override for deterministic tests")
    parser.add_argument("--out", type=Path, help="write the preflight artifact to this path")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    latest = load_latest_metadata(args.latest_metadata)
    source_commit = args.source_commit or git_capture(repo_root, ["rev-parse", "HEAD"])
    workspace_version = args.workspace_version or workspace_package_version(repo_root)
    existing_latest_tag = args.existing_latest_tag or latest.get("tag")
    existing_latest_url = args.existing_latest_url or latest.get("url")
    tag_commit = args.tag_commit
    if tag_commit is None:
        tag_commit = local_tag_commit(repo_root, args.release_tag)
    dirty_entries = dirty_tree_entries(repo_root, args.dirty_status, args.dirty_entry)
    generated_at = args.generated_at or datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")

    artifact = evaluate_preflight(
        release_tag=args.release_tag,
        source_commit=source_commit,
        workspace_package_version=workspace_version,
        tag_commit=tag_commit,
        existing_latest_tag=existing_latest_tag,
        existing_latest_url=existing_latest_url,
        missing_installer_latest_tag=args.missing_installer_latest_tag,
        dirty_entries=dirty_entries,
        generated_at=generated_at,
    )
    if args.out:
        write_json(args.out, artifact)
    if artifact["decision"]["ok"]:
        print(f"current latest repair preflight ok: target_tag={args.release_tag}")
        return 0
    for diagnostic in artifact["decision"]["diagnostics"]:
        print(
            "current latest repair preflight failed: "
            f"field={diagnostic['field']} expected={diagnostic['expected']} "
            f"observed={diagnostic['observed']} safe_repair_action={diagnostic['safe_repair_action']}",
            file=sys.stderr,
        )
    return 1


def evaluate_preflight(
    *,
    release_tag: str,
    source_commit: str,
    workspace_package_version: str,
    tag_commit: str | None,
    existing_latest_tag: str | None,
    existing_latest_url: str | None,
    missing_installer_latest_tag: str,
    dirty_entries: list[str],
    generated_at: str,
) -> dict:
    expected_release_tag = expected_tag(workspace_package_version)
    diagnostics: list[Diagnostic] = []

    if not COMMIT_RE.fullmatch(source_commit):
        diagnostics.append(
            Diagnostic(
                "source_commit",
                "40-character lowercase git commit SHA",
                source_commit,
                "rerun release workflow from a checked-out commit",
            )
        )
    if STABLE_TAG_RE.fullmatch(release_tag) is None:
        diagnostics.append(
            Diagnostic(
                "target_tag",
                "stable tag vMAJOR.MINOR.PATCH",
                release_tag,
                "cut a new stable tag and rerun release workflow",
            )
        )
    if release_tag != expected_release_tag:
        diagnostics.append(
            Diagnostic(
                "target_tag",
                expected_release_tag,
                release_tag,
                "cut a new stable tag matching Cargo.toml workspace.package.version, then rerun release workflow",
            )
        )
    if dirty_entries:
        diagnostics.append(
            Diagnostic(
                "dirty_tree",
                "clean",
                "; ".join(dirty_entries),
                "commit or discard dirty workspace changes before cutting the release tag",
            )
        )
    if tag_commit is not None and tag_commit != source_commit:
        diagnostics.append(
            Diagnostic(
                "tag_commit",
                source_commit,
                tag_commit,
                "stop for manual release-state repair; do not upload current-main assets into an existing tag",
            )
        )

    existing_latest_semver = parse_stable_tag(existing_latest_tag) if existing_latest_tag else None
    if existing_latest_tag is None:
        diagnostics.append(
            Diagnostic(
                "existing_latest_tag",
                "current public latest stable tag",
                "missing",
                "rerun after GitHub latest metadata is readable; do not repair latest from an unknown release state",
            )
        )
    elif existing_latest_semver is None:
        diagnostics.append(
            Diagnostic(
                "existing_latest_tag",
                "stable tag vMAJOR.MINOR.PATCH",
                existing_latest_tag,
                "stop for manual release-state repair before publishing installer assets",
            )
        )

    release_order = release_order_state(release_tag, existing_latest_tag)
    if existing_latest_semver is not None and release_order["candidate_is_newer_than_existing_latest"] is False:
        diagnostics.append(
            Diagnostic(
                "release_order",
                f">{existing_latest_tag}",
                release_tag,
                "cut a new stable tag greater than the current public latest and rerun release workflow; do not backfill old release assets",
            )
        )

    ok = not diagnostics
    supersedes_missing_installer = (
        ok
        and existing_latest_tag == missing_installer_latest_tag
        and release_order["candidate_is_newer_than_existing_latest"] is True
    )
    tag_state = "absent"
    if tag_commit is not None:
        tag_state = "matches_source" if tag_commit == source_commit else "mismatch"

    return {
        "schema_version": SCHEMA_VERSION,
        "kind": "m80_current_latest_repair_preflight",
        "generated_at": generated_at,
        "target_tag": release_tag,
        "source_commit": source_commit,
        "workspace_package_version": workspace_package_version,
        "expected_release_tag": expected_release_tag,
        "dirty_tree": {
            "dirty": bool(dirty_entries),
            "entries": dirty_entries,
        },
        "tag": {
            "state": tag_state,
            "commit": tag_commit,
        },
        "existing_latest": {
            "tag": existing_latest_tag,
            "url": existing_latest_url,
        },
        "missing_installer_latest_tag": missing_installer_latest_tag,
        "supersedes_missing_installer_latest_state": supersedes_missing_installer,
        "release_order": release_order,
        "decision": {
            "ok": ok,
            "status": "accepted" if ok else "rejected",
            "safe_repair_action": "rerun release workflow for the accepted target tag" if ok else "repair diagnostics first",
            "diagnostics": [diagnostic.to_json() for diagnostic in diagnostics],
        },
    }


def release_order_state(release_tag: str, existing_latest_tag: str | None) -> dict:
    candidate = parse_stable_tag(release_tag)
    existing = parse_stable_tag(existing_latest_tag) if existing_latest_tag else None
    if candidate is None or existing is None:
        return {
            "candidate_is_newer_than_existing_latest": None,
            "candidate_semver": list(candidate) if candidate else None,
            "existing_latest_semver": list(existing) if existing else None,
        }
    return {
        "candidate_is_newer_than_existing_latest": candidate > existing,
        "candidate_semver": list(candidate),
        "existing_latest_semver": list(existing),
    }


def parse_stable_tag(tag: str | None) -> tuple[int, int, int] | None:
    if tag is None:
        return None
    match = STABLE_TAG_RE.fullmatch(tag)
    if match is None:
        return None
    return (
        int(match.group("major")),
        int(match.group("minor")),
        int(match.group("patch")),
    )


def expected_tag(workspace_version: str) -> str:
    if WORKSPACE_VERSION_RE.fullmatch(workspace_version) is None:
        return f"v{workspace_version}"
    return f"v{workspace_version}"


def workspace_package_version(repo_root: Path) -> str:
    cargo_toml = repo_root / "Cargo.toml"
    with cargo_toml.open("rb") as f:
        cargo = tomllib.load(f)
    version = cargo.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or not version:
        raise SystemExit(f"workspace package version missing from {cargo_toml}")
    return version


def local_tag_commit(repo_root: Path, release_tag: str) -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "--verify", f"refs/tags/{release_tag}^{{commit}}"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def dirty_tree_entries(repo_root: Path, dirty_status: str, dirty_entries: list[str]) -> list[str]:
    if dirty_status == "clean":
        return []
    if dirty_status == "dirty":
        return dirty_entries or ["fixture dirty entry"]
    result = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr.strip() or "git status failed")
    return [line for line in result.stdout.splitlines() if line]


def git_capture(repo_root: Path, args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result.stdout.strip()


def load_latest_metadata(path: Path | None) -> dict[str, str | None]:
    if path is None:
        return {"tag": None, "url": None}
    with path.open() as f:
        payload = json.load(f)
    if not isinstance(payload, dict):
        raise SystemExit(f"latest metadata must be a JSON object: {path}")
    tag = payload.get("tag_name") or payload.get("tagName")
    url = payload.get("html_url") or payload.get("url")
    return {
        "tag": tag if isinstance(tag, str) and tag else None,
        "url": url if isinstance(url, str) and url else None,
    }


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
