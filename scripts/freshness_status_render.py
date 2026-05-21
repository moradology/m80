#!/usr/bin/env python3
"""Render README/runbook public installer status markers from freshness status."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
from typing import Any

from freshness_status import validate_freshness_status
from quickstart_snippets import extract_marked_quickstart_snippets_from_text as extract_quickstart_snippets_from_text


MARKER_RE = re.compile(r"^<!--\s*m80:freshness-status\s+(start|end)\s*-->$")
DEFAULT_STATUS = Path("docs/behaviors/release/freshness-status-docs.json")
DEFAULT_DOCS = (Path("README.md"), Path("docs/runbook/release.md"))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--status", type=Path, default=DEFAULT_STATUS, help="freshness status JSON input")
    parser.add_argument(
        "--artifact-root",
        type=Path,
        help="directory containing relative proof artifact paths; defaults to the status file directory",
    )
    parser.add_argument("--docs-root", type=Path, default=Path("."), help="repository root for docs and links")
    parser.add_argument("--doc", action="append", type=Path, help="doc to update; defaults to README and release runbook")
    parser.add_argument("--check", action="store_true", help="fail instead of writing when marker output is stale")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    docs_root = args.docs_root.resolve()
    status_path = args.status.resolve()
    artifact_root = (args.artifact_root or args.status.parent).resolve()
    status = validate_freshness_status(status_path, artifact_root=artifact_root, docs_root=docs_root)
    docs = args.doc or list(DEFAULT_DOCS)

    stale: list[Path] = []
    for doc in docs:
        doc_path = (docs_root / doc).resolve() if not doc.is_absolute() else doc.resolve()
        original = doc_path.read_text()
        rendered = replace_status_marker(
            original,
            render_status_text(status, artifact_root=artifact_root, docs_root=docs_root, doc_path=doc_path),
            doc_path=doc_path,
        )
        ensure_quickstart_snippets_unchanged(original, rendered, doc_path=doc_path)
        if rendered == original:
            continue
        stale.append(doc_path)
        if not args.check:
            doc_path.write_text(rendered)

    if stale and args.check:
        for doc in stale:
            print(f"freshness status marker is stale: {doc}", flush=True)
        return 1

    verb = "checked" if args.check else "rendered"
    print(f"{verb} freshness status markers: {', '.join(str(doc) for doc in docs)}")
    return 0


def replace_status_marker(text: str, replacement: str, *, doc_path: Path) -> str:
    lines = text.splitlines()
    output: list[str] = []
    in_marker = False
    seen_blocks = 0
    for index, line in enumerate(lines, start=1):
        marker = MARKER_RE.fullmatch(line.strip())
        if marker is None:
            if not in_marker:
                output.append(line)
            continue

        kind = marker.group(1)
        if kind == "start":
            if in_marker:
                raise SystemExit(f"{doc_path}:{index}: nested m80:freshness-status marker")
            in_marker = True
            seen_blocks += 1
            if seen_blocks > 1:
                raise SystemExit(f"{doc_path}:{index}: duplicate m80:freshness-status marker")
            output.append(line)
            output.extend(replacement.splitlines())
            continue

        if not in_marker:
            raise SystemExit(f"{doc_path}:{index}: unmatched m80:freshness-status end marker")
        in_marker = False
        output.append(line)

    if in_marker:
        raise SystemExit(f"{doc_path}: missing m80:freshness-status end marker")
    if seen_blocks == 0:
        raise SystemExit(f"{doc_path}: missing m80:freshness-status marker")
    return "\n".join(output) + "\n"


def ensure_quickstart_snippets_unchanged(original: str, rendered: str, *, doc_path: Path) -> None:
    before = extract_doc_quickstart_snippets(original, doc_path=doc_path)
    after = extract_doc_quickstart_snippets(rendered, doc_path=doc_path)
    if before != after:
        raise SystemExit(f"{doc_path}: freshness status renderer changed a quickstart snippet")


def extract_doc_quickstart_snippets(text: str, *, doc_path: Path) -> dict[str, str]:
    return extract_quickstart_snippets_from_text(text, source=str(doc_path))


def render_status_text(
    status: dict[str, Any],
    *,
    artifact_root: Path,
    docs_root: Path,
    doc_path: Path,
) -> str:
    status_value = status["status"]
    resolved = status["resolved_latest_tag"]
    expected = status["expected_highest_stable_tag"]
    links = proof_links(status, artifact_root=artifact_root, docs_root=docs_root, doc_path=doc_path)

    if status_value == "pending":
        return (
            "Public installer status: pending public proof. The latest command below is the "
            "release-channel template; do not treat it as publicly proven until the freshness "
            "status becomes `public_green`."
        )
    if status_value == "public_green":
        return (
            f"Public installer status: public proof green for `{resolved}`. Latest and pinned "
            f"install URLs were verified from unauthenticated public release assets. Proof: {links}."
        )
    if status_value == "scaffolded":
        return (
            f"Public installer status: scaffolded fixture proof for `{resolved}`. This is not "
            f"public release proof. Fixture proof: {links}."
        )
    if status_value == "stale":
        return (
            f"Public installer status: stale for `{resolved}`; expected `{expected}`. Do not treat "
            f"latest as publicly proven. Evidence: {links}."
        )
    if status_value == "failed":
        previous = previous_public_green_link(status, artifact_root=artifact_root, docs_root=docs_root, doc_path=doc_path)
        if previous is None:
            previous = "no previous public green proof is linked by this status"
        return (
            f"Public installer status: failed for `{resolved}`. Do not treat latest as publicly "
            f"proven. Previous public green: {previous}."
        )
    raise AssertionError(f"unhandled freshness status: {status_value}")


def proof_links(status: dict[str, Any], *, artifact_root: Path, docs_root: Path, doc_path: Path) -> str:
    links = [
        markdown_artifact_link(row, artifact_root=artifact_root, docs_root=docs_root, doc_path=doc_path)
        for row in status["proof_artifacts"]
    ]
    if not links:
        return "no proof artifact"
    return ", ".join(links)


def previous_public_green_link(
    status: dict[str, Any],
    *,
    artifact_root: Path,
    docs_root: Path,
    doc_path: Path,
) -> str | None:
    for row in status["proof_artifacts"]:
        if row["artifact_class"] == "public" and "previous" in row["kind"] and "green" in row["kind"]:
            return markdown_artifact_link(row, artifact_root=artifact_root, docs_root=docs_root, doc_path=doc_path)
    return None


def markdown_artifact_link(row: dict[str, Any], *, artifact_root: Path, docs_root: Path, doc_path: Path) -> str:
    artifact = (artifact_root / row["path"]).resolve()
    try:
        artifact.relative_to(docs_root)
    except ValueError as exc:
        raise SystemExit(f"freshness proof artifact is outside docs root and cannot be linked: {artifact}") from exc
    target = os.path.relpath(artifact, start=doc_path.parent)
    return f"[{row['kind']}]({target})"


if __name__ == "__main__":
    raise SystemExit(main())
