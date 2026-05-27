#!/usr/bin/env python3
"""Verify and capture the C7 GitHub Release notes proof."""

from __future__ import annotations

import argparse
from datetime import UTC, datetime
import importlib.util
from pathlib import Path
import re
import subprocess
import sys
from types import ModuleType


REPO_ROOT = Path(__file__).resolve().parents[1]
EXTRACT_SCRIPT = REPO_ROOT / "scripts" / "extract-release-notes.py"
TAG_RE = re.compile(r"^v\d+\.\d+\.\d+$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
WORKFLOW_RUN_URL_RE = re.compile(r"^https://github\.com/[^/]+/[^/]+/actions/runs/[0-9]+$")
CATEGORY_RE = re.compile(
    r"^### (Added|Changed|Deprecated|Removed|Fixed|Security|Performance|Documented)\s*$",
    re.MULTILINE,
)
FALLBACK_BODY_RE = re.compile(r"release artifacts from .*/actions/runs/")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify a GitHub Release body against the committed CHANGELOG.md section",
    )
    parser.add_argument("--tag", required=True)
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument(
        "--release-body-file",
        type=Path,
        help="Read the GitHub Release body from a file instead of calling gh",
    )
    parser.add_argument("--repo", default="moradology/m80")
    parser.add_argument("--gh-bin", default="gh")
    parser.add_argument("--workflow-run-url", required=True)
    parser.add_argument("--commit-sha")
    parser.add_argument("--human-reviewed-by", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def load_extractor() -> ModuleType:
    spec = importlib.util.spec_from_file_location("extract_release_notes", EXTRACT_SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load extractor: {EXTRACT_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def current_commit_sha() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        check=True,
        text=True,
        capture_output=True,
    )
    return result.stdout.strip()


def release_body(args: argparse.Namespace) -> str:
    if args.release_body_file is not None:
        return args.release_body_file.read_text(encoding="utf-8")
    result = subprocess.run(
        [
            args.gh_bin,
            "release",
            "view",
            args.tag,
            "--repo",
            args.repo,
            "--json",
            "body",
            "--jq",
            ".body",
        ],
        check=True,
        text=True,
        capture_output=True,
    )
    return result.stdout


def validate_inputs(args: argparse.Namespace, commit_sha: str) -> list[str]:
    errors: list[str] = []
    if TAG_RE.match(args.tag) is None:
        errors.append(f"--tag must be shaped vX.Y.Z: {args.tag}")
    if COMMIT_RE.match(commit_sha) is None:
        errors.append(f"--commit-sha must be a 40-character lowercase hex SHA: {commit_sha}")
    if WORKFLOW_RUN_URL_RE.match(args.workflow_run_url) is None:
        errors.append(
            "--workflow-run-url must be a GitHub Actions run URL like "
            "https://github.com/OWNER/REPO/actions/runs/123"
        )
    if not args.human_reviewed_by.strip():
        errors.append("--human-reviewed-by must name the human reviewer")
    return errors


def validate_body(changelog_notes: str, body: str) -> list[str]:
    errors: list[str] = []
    expected = changelog_notes.strip()
    actual = body.strip()
    if not expected:
        errors.append("CHANGELOG.md has no reviewed release section for the target tag")
        return errors
    if actual != expected:
        errors.append("GitHub Release body does not exactly match the CHANGELOG.md section body")
    if len(actual) <= 200:
        errors.append(f"GitHub Release body is too short: {len(actual)} characters")
    if CATEGORY_RE.search(actual) is None:
        errors.append("GitHub Release body has no Keep-a-Changelog category heading")
    if FALLBACK_BODY_RE.search(actual) is not None:
        errors.append("GitHub Release body still matches the old one-line fallback")
    return errors


def markdown_fence(text: str) -> str:
    longest = max((len(match.group(0)) for match in re.finditer(r"`+", text)), default=0)
    return "`" * max(3, longest + 1)


def render_proof(
    *,
    tag: str,
    repo: str,
    commit_sha: str,
    workflow_run_url: str,
    human_reviewed_by: str,
    release_body_text: str,
) -> str:
    now = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
    body = release_body_text.strip()
    fence = markdown_fence(body)
    command = f"gh release view {tag} --repo {repo} --json body --jq .body"
    return (
        f"# Release Notes Proof: {tag}\n\n"
        f"- target_tag: `{tag}`\n"
        f"- source_commit: `{commit_sha}`\n"
        f"- workflow_run_url: {workflow_run_url}\n"
        f"- release_body_command: `{command}`\n"
        f"- human_reviewed_by: `{human_reviewed_by}`\n"
        f"- verified_at_utc: `{now}`\n\n"
        "## Result\n\n"
        "The GitHub Release body exactly matched the committed, human-reviewed "
        "`CHANGELOG.md` section body for the target tag. It is longer than 200 "
        "characters, includes a Keep-a-Changelog category heading, and does not "
        "match the old one-line release-artifacts fallback.\n\n"
        "## GitHub Release Body\n\n"
        f"{fence}markdown\n{body}\n{fence}\n"
    )


def main() -> int:
    args = parse_args()
    commit_sha = args.commit_sha or current_commit_sha()
    input_errors = validate_inputs(args, commit_sha)
    if input_errors:
        for error in input_errors:
            print(f"::error::{error}", file=sys.stderr)
        return 2

    extractor = load_extractor()
    changelog_text = args.changelog.read_text(encoding="utf-8")
    changelog_notes = extractor.extract_release_notes(changelog_text, args.tag)
    try:
        body = release_body(args)
    except subprocess.CalledProcessError as exc:
        stderr = exc.stderr.strip() if exc.stderr else str(exc)
        print(f"::error::failed to read GitHub Release body: {stderr}", file=sys.stderr)
        return 3

    body_errors = validate_body(changelog_notes, body)
    if body_errors:
        for error in body_errors:
            print(f"::error::{error}", file=sys.stderr)
        return 1

    proof = render_proof(
        tag=args.tag,
        repo=args.repo,
        commit_sha=commit_sha,
        workflow_run_url=args.workflow_run_url,
        human_reviewed_by=args.human_reviewed_by.strip(),
        release_body_text=body,
    )
    if args.dry_run:
        print(proof, end="")
        return 0
    args.out.write_text(proof, encoding="utf-8")
    print(f"release notes proof written: {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
