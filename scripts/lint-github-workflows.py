#!/usr/bin/env python3
"""Fail closed on GitHub workflow authority-policy drift."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys


FULL_SHA_RE = re.compile(r"[0-9a-fA-F]{40}")
USES_RE = re.compile(r"^\s*(?:-\s*)?uses:\s*([^#\s]+)")
JOB_RE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$")
PERMISSION_RE = re.compile(r"^\s{6}([A-Za-z0-9_-]+):\s*([A-Za-z0-9_-]+)\s*(?:#.*)?$")
SECRET_RE = re.compile(r"\$\{\{\s*secrets\.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--workflow-dir",
        type=Path,
        default=Path(".github/workflows"),
        help="directory containing GitHub workflow YAML files",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    errors = lint_workflow_dir(args.workflow_dir)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"workflow policy ok: {args.workflow_dir}")
    return 0


def lint_workflow_dir(workflow_dir: Path) -> list[str]:
    files = sorted(set(workflow_dir.glob("*.yml")) | set(workflow_dir.glob("*.yaml")))
    if not files:
        return [f"{workflow_dir}: no workflow files found"]
    errors: list[str] = []
    for path in files:
        text = path.read_text()
        lines = text.splitlines()
        release_workflow = workflow_needs_release_guards(path)
        errors.extend(lint_action_refs(path, lines))
        errors.extend(lint_top_level_permissions(path, lines))
        errors.extend(lint_job_permissions(path, lines, release_workflow=release_workflow))
        if release_workflow:
            errors.extend(lint_release_concurrency(path, lines))
        if has_pull_request_event(lines) and SECRET_RE.search(text):
            errors.append(f"{path}: pull_request workflow must not reference secrets.*")
    return errors


def lint_action_refs(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for line_no, line in enumerate(lines, start=1):
        match = USES_RE.match(line)
        if not match:
            continue
        action = strip_quotes(match.group(1))
        if action.startswith("./") or action.startswith("docker://"):
            continue
        if "@" not in action:
            errors.append(f"{path}:{line_no}: action ref missing @ pin: {action}")
            continue
        repo, ref = action.rsplit("@", 1)
        if is_trusted_first_party_action(repo):
            continue
        if not FULL_SHA_RE.fullmatch(ref):
            errors.append(
                f"{path}:{line_no}: third-party action must use a full commit SHA: {action}"
            )
    return errors


def lint_top_level_permissions(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for index, line in enumerate(lines):
        if not line.startswith("permissions:"):
            continue
        suffix = line.split(":", 1)[1].strip()
        if suffix:
            if suffix != "{}":
                errors.append(f"{path}:{index + 1}: top-level permissions must be an explicit read map, not {suffix}")
            continue
        for offset, nested in enumerate(lines[index + 1 :], start=index + 2):
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if not nested.startswith("  "):
                break
            key_value = nested.strip().split(":", 1)
            if len(key_value) != 2:
                continue
            key, value = key_value[0], key_value[1].strip()
            if is_write_permission(value):
                errors.append(
                    f"{path}:{offset}: top-level permissions must not grant {key}: {value}"
                )
    return errors


def lint_job_permissions(
    path: Path,
    lines: list[str],
    *,
    release_workflow: bool,
) -> list[str]:
    errors: list[str] = []
    for job_id, start, end in job_blocks(lines):
        permissions = job_permissions(lines[start:end])
        if release_workflow and permissions is None:
            errors.append(f"{path}:{start + 1}: release job {job_id} must declare permissions")
            continue
        if permissions is None:
            continue
        for scope, level in permissions.items():
            if not is_write_permission(level):
                continue
            if scope == "contents" and is_publish_job(job_id):
                continue
            errors.append(
                f"{path}:{start + 1}: job {job_id} must not grant {scope}: {level}"
            )
    return errors


def lint_release_concurrency(path: Path, lines: list[str]) -> list[str]:
    group = top_level_concurrency_group(lines)
    if group is None:
        return [f"{path}: release workflow must declare top-level concurrency.group"]
    if "github.ref_name" not in group and "github.ref" not in group and "latest" not in group:
        return [
            f"{path}: release concurrency group must be keyed by tag/ref or latest promotion target"
        ]
    return []


def job_blocks(lines: list[str]) -> list[tuple[str, int, int]]:
    jobs_start = next((i for i, line in enumerate(lines) if line == "jobs:"), None)
    if jobs_start is None:
        return []
    jobs: list[tuple[str, int, int]] = []
    current: tuple[str, int] | None = None
    for index in range(jobs_start + 1, len(lines)):
        line = lines[index]
        if line and not line.startswith(" "):
            break
        match = JOB_RE.match(line)
        if not match:
            continue
        if current is not None:
            jobs.append((current[0], current[1], index))
        current = (match.group(1), index)
    if current is not None:
        jobs.append((current[0], current[1], len(lines)))
    return jobs


def job_permissions(lines: list[str]) -> dict[str, str] | None:
    for index, line in enumerate(lines):
        if not line.startswith("    permissions:"):
            continue
        suffix = line.split(":", 1)[1].strip()
        if suffix:
            return {"*": suffix}
        permissions: dict[str, str] = {}
        for nested in lines[index + 1 :]:
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if not nested.startswith("      "):
                break
            match = PERMISSION_RE.match(nested)
            if match:
                permissions[match.group(1)] = match.group(2)
        return permissions
    return None


def top_level_concurrency_group(lines: list[str]) -> str | None:
    for index, line in enumerate(lines):
        if line != "concurrency:":
            continue
        for nested in lines[index + 1 :]:
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if not nested.startswith("  "):
                break
            stripped = nested.strip()
            if stripped.startswith("group:"):
                return stripped.split(":", 1)[1].strip()
        return None
    return None


def has_pull_request_event(lines: list[str]) -> bool:
    for index, line in enumerate(lines):
        if line.startswith("on:") and "pull_request" in line:
            return True
        if line.startswith("on:"):
            for nested in lines[index + 1 :]:
                if not nested.strip() or nested.lstrip().startswith("#"):
                    continue
                if not nested.startswith("  "):
                    break
                if nested.strip().startswith("pull_request:"):
                    return True
    return False


def is_trusted_first_party_action(repo: str) -> bool:
    return repo.startswith("actions/")


def workflow_needs_release_guards(path: Path) -> bool:
    return "release" in path.name or "latest" in path.name


def is_publish_job(job_id: str) -> bool:
    return job_id == "publish" or job_id.startswith("publish-")


def is_write_permission(level: str) -> bool:
    return level in {"write", "write-all"}


def strip_quotes(value: str) -> str:
    if (value.startswith('"') and value.endswith('"')) or (
        value.startswith("'") and value.endswith("'")
    ):
        return value[1:-1]
    return value


if __name__ == "__main__":
    raise SystemExit(main())
