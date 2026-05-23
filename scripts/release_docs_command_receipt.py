#!/usr/bin/env python3
"""Build and verify the docs-command release-readiness receipt."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
from typing import Any

from quickstart_snippets import (
    RAW_MAIN_INSTALL_RE,
    ARTIFACT_ONLY_LATEST_RE,
    expected_quickstart_snippets,
    extract_marked_quickstart_snippets,
    install_snippets,
)
from release_url_contract import latest_install_command, release_repository


SCHEMA_VERSION = 1
KIND = "m80_release_readiness_docs_command"
LANE_ID = "docs-command"
LANE_KIND = "docs-command"
PROOF_KIND = "docs-command-receipt"
SUBSTRATE_KIND = "github-actions"
SHA256_PREFIX = "sha256:"
DEFAULT_RENDERER = Path("scripts/render-release-install-snippets.py")
README_SNIPPETS = {
    "latest-install",
    "post-install-smoke",
    "freshness-check",
    "pinned-install",
    "repair-status",
}


class VerificationError(ValueError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--repository", default="moradology/m80")
    parser.add_argument("--release-tag", required=True)
    parser.add_argument("--commit-sha", required=True)
    parser.add_argument("--workflow-run-id")
    parser.add_argument("--renderer", type=Path, default=DEFAULT_RENDERER)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--expected-release-tag")
    parser.add_argument("--expected-commit-sha")
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.write:
            receipt = build_receipt(args)
            write_json(args.out, receipt)
        receipt = read_json(args.out, "docs-command receipt")
        verify_receipt(
            receipt,
            root=args.root,
            repository=args.repository,
            release_tag=args.expected_release_tag or args.release_tag,
            commit_sha=args.expected_commit_sha or args.commit_sha,
            workflow_run_id=args.workflow_run_id,
            renderer=args.renderer.resolve(),
        )
    except VerificationError as exc:
        raise SystemExit(str(exc)) from exc
    print(f"docs-command release readiness receipt ok: {args.out}")
    return 0


def build_receipt(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    check(args.repository == release_repository(), f"repository mismatch: expected {args.repository}, got {release_repository()}")
    snippet_sources = collect_snippet_sources(root)
    renderer = args.renderer.resolve()
    rendered = rendered_install_commands(renderer, args.release_tag, cwd=root)
    expected_rendered = expected_rendered_install_commands(args.release_tag)
    check(rendered == expected_rendered, "generated install snippet drift")
    check(latest_install_command() in rendered, "generated latest install command missing")

    digest_payload = {
        "repository": args.repository,
        "release_tag": args.release_tag,
        "snippet_sources": snippet_sources,
        "rendered_install_commands": rendered,
    }
    digest = sha256_json(digest_payload)
    receipt = {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "lane_id": LANE_ID,
        "lane_kind": LANE_KIND,
        "proof_kind": PROOF_KIND,
        "status": "passed",
        "release_tag": args.release_tag,
        "commit_sha": args.commit_sha,
        "workflow_run_id": args.workflow_run_id,
        "verification_time": utc_now(),
        "substrate": {
            "kind": SUBSTRATE_KIND,
            "fixture": False,
        },
        "repository": args.repository,
        "snippet_sources": snippet_sources,
        "rendered_install_commands": rendered,
        "command_digest_sha256": digest,
        "remediation": {
            "command": "python3 scripts/release_docs_command_receipt.py",
            "bead_id": None,
        },
    }
    verify_receipt(
        receipt,
        root=root,
        repository=args.repository,
        release_tag=args.release_tag,
        commit_sha=args.commit_sha,
        workflow_run_id=args.workflow_run_id,
        renderer=renderer,
    )
    return receipt


def verify_receipt(
    receipt: dict[str, Any],
    *,
    root: Path,
    repository: str,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str | None,
    renderer: Path,
) -> None:
    require_exact_fields(
        receipt,
        {
            "schema_version",
            "kind",
            "lane_id",
            "lane_kind",
            "proof_kind",
            "status",
            "release_tag",
            "commit_sha",
            "workflow_run_id",
            "verification_time",
            "substrate",
            "repository",
            "snippet_sources",
            "rendered_install_commands",
            "command_digest_sha256",
            "remediation",
        },
        "docs-command receipt",
    )
    check(receipt["schema_version"] == SCHEMA_VERSION, "unsupported docs-command receipt schema_version")
    check(receipt["kind"] == KIND, "docs-command receipt kind mismatch")
    check(receipt["lane_id"] == LANE_ID, "docs-command receipt lane_id mismatch")
    check(receipt["lane_kind"] == LANE_KIND, "docs-command receipt lane_kind mismatch")
    check(receipt["proof_kind"] == PROOF_KIND, "docs-command receipt proof_kind mismatch")
    check(receipt["status"] == "passed", "docs-command receipt status must be passed")
    check(receipt["release_tag"] == release_tag, "stale release tag")
    check(receipt["commit_sha"] == commit_sha, "stale commit sha")
    if workflow_run_id is not None:
        check(receipt["workflow_run_id"] == workflow_run_id, "stale workflow run id")
    check(receipt["repository"] == repository, "repository mismatch")
    check(repository == release_repository(), f"repository mismatch: expected {repository}, got {release_repository()}")

    substrate = require_object(receipt["substrate"], "substrate")
    require_exact_fields(substrate, {"kind", "fixture"}, "substrate")
    check(substrate["kind"] == SUBSTRATE_KIND, "docs-command substrate mismatch")
    check(substrate["fixture"] is False, "docs-command receipt cannot be fixture-backed")

    expected_sources = collect_snippet_sources(root.resolve())
    check(receipt["snippet_sources"] == expected_sources, "quickstart snippet source drift")
    expected_rendered = rendered_install_commands(renderer, release_tag, cwd=root.resolve())
    check(receipt["rendered_install_commands"] == expected_rendered, "generated install snippet drift")
    digest_payload = {
        "repository": repository,
        "release_tag": release_tag,
        "snippet_sources": expected_sources,
        "rendered_install_commands": expected_rendered,
    }
    check(receipt["command_digest_sha256"] == sha256_json(digest_payload), "command digest mismatch")

    remediation = require_object(receipt["remediation"], "remediation")
    require_exact_fields(remediation, {"command", "bead_id"}, "remediation")
    check(remediation["command"] == "python3 scripts/release_docs_command_receipt.py", "docs-command remediation mismatch")


def collect_snippet_sources(root: Path) -> list[dict[str, Any]]:
    expected = expected_quickstart_snippets()
    docs = [
        ("README.md", {name: expected[name] for name in sorted(README_SNIPPETS)}),
        ("docs/runbook/release.md", expected),
    ]
    rows: list[dict[str, Any]] = []
    for relative, expected_snippets in docs:
        path = root / relative
        check(path.is_file(), f"quickstart snippet source missing: {relative}")
        text = path.read_text()
        check(RAW_MAIN_INSTALL_RE.search(text) is None, f"{relative}: raw main install URL is forbidden")
        check(ARTIFACT_ONLY_LATEST_RE.search(text) is None, f"{relative}: artifact-only latest URL is forbidden")
        try:
            snippets = extract_marked_quickstart_snippets(path)
        except ValueError as exc:
            raise VerificationError(str(exc)) from exc
        check(snippets == expected_snippets, f"{relative}: quickstart snippet drift")
        rows.append(
            {
                "path": relative,
                "snippets": [
                    {
                        "name": name,
                        "command": snippets[name],
                        "sha256": sha256_text(snippets[name]),
                    }
                    for name in sorted(snippets)
                ],
            }
        )
    return rows


def rendered_install_commands(renderer: Path, release_tag: str, *, cwd: Path) -> list[str]:
    command = ["python3", str(renderer), "--release-tag", release_tag]
    result = subprocess.run(command, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode != 0:
        raise VerificationError(f"generated install snippet renderer failed: {result.stderr.strip()}")
    return result.stdout.rstrip("\n").splitlines()


def expected_rendered_install_commands(release_tag: str) -> list[str]:
    latest, pinned, verified = install_snippets(release_tag)
    return [latest.body, pinned.body, "", *verified.body.splitlines()]


def sha256_json(value: object) -> str:
    data = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return f"{SHA256_PREFIX}{hashlib.sha256(data).hexdigest()}"


def sha256_text(value: str) -> str:
    return f"{SHA256_PREFIX}{hashlib.sha256(value.encode()).hexdigest()}"


def read_json(path: Path, label: str) -> dict[str, Any]:
    try:
        with path.open() as f:
            value = json.load(f)
    except FileNotFoundError as exc:
        raise VerificationError(f"{label} missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise VerificationError(f"{label} invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise VerificationError(f"{label} must be a JSON object")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def require_exact_fields(obj: dict[str, Any], fields: set[str], label: str) -> None:
    actual = set(obj)
    missing = sorted(fields - actual)
    extra = sorted(actual - fields)
    if missing:
        raise VerificationError(f"{label}: missing field(s): {', '.join(missing)}")
    if extra:
        raise VerificationError(f"{label}: unknown field(s): {', '.join(extra)}")


def require_object(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise VerificationError(f"{label} must be an object")
    return value


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def check(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


if __name__ == "__main__":
    raise SystemExit(main())
