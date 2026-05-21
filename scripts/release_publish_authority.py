#!/usr/bin/env python3
"""Verify release publish authority before mutating GitHub release state."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import re
import sys


PUBLISH_AUTHORITY_POLICY = {
    "repository": "moradology/m80",
    "workflow_path": ".github/workflows/release-artifacts.yml",
    "publish_job_id": "publish-release-artifacts",
    "build_job_id": "build-release-artifacts",
    "allowed_ref_shape": "refs/tags/v*",
    "token_source": "github.token",
    "token_env": "GH_TOKEN",
    "top_level_permissions": {"contents": "read"},
    "build_job_permissions": {
        "contents": "read",
        "id-token": "write",
        "attestations": "write",
    },
    "publish_job_permissions": {"contents": "write"},
}

JOB_RE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$")
PERMISSION_RE = re.compile(r"^\s{6}([A-Za-z0-9_-]+):\s*([A-Za-z0-9_-]+)\s*(?:#.*)?$")
VERSION_TAG_REF_RE = re.compile(r"^refs/tags/v[A-Za-z0-9][A-Za-z0-9._+-]*$")


@dataclass(frozen=True)
class PublishContext:
    repository: str
    github_ref: str
    workflow_ref: str
    github_job: str
    token_source: str
    token_env: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--workflow-file",
        type=Path,
        default=Path(PUBLISH_AUTHORITY_POLICY["workflow_path"]),
        help="release workflow file to verify",
    )
    parser.add_argument("--repository", default=None)
    parser.add_argument("--github-ref", default=None)
    parser.add_argument("--workflow-ref", default=None)
    parser.add_argument("--github-job", default=None)
    parser.add_argument("--token-source", default=None)
    parser.add_argument(
        "--token-env",
        default=PUBLISH_AUTHORITY_POLICY["token_env"],
        help="environment variable expected to hold the publish token",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    context = PublishContext(
        repository=resolve_value(args.repository, "GITHUB_REPOSITORY", "repository"),
        github_ref=resolve_value(args.github_ref, "GITHUB_REF", "github-ref"),
        workflow_ref=resolve_value(args.workflow_ref, "GITHUB_WORKFLOW_REF", "github-workflow-ref"),
        github_job=resolve_value(args.github_job, "GITHUB_JOB", "github-job"),
        token_source=resolve_value(args.token_source, "M80_RELEASE_TOKEN_SOURCE", "token-source"),
        token_env=args.token_env,
    )
    verify_context(context)
    verify_workflow_policy(args.workflow_file)
    print(
        "release publish authority ok: "
        f"{context.repository} {context.github_ref} {context.github_job}"
    )
    return 0


def resolve_value(value: str | None, env_name: str, label: str) -> str:
    observed = value if value is not None else os.environ.get(env_name, "")
    require(observed.strip() != "", f"release publish authority {label} missing")
    return observed


def verify_context(context: PublishContext) -> None:
    policy = PUBLISH_AUTHORITY_POLICY
    require(
        context.repository == policy["repository"],
        "release publish authority repository mismatch: "
        f"expected {policy['repository']} got {context.repository}",
    )
    require(
        VERSION_TAG_REF_RE.fullmatch(context.github_ref) is not None,
        "release publish authority github-ref must match refs/tags/v*",
    )
    expected_workflow_ref = (
        f"{policy['repository']}/{policy['workflow_path']}@{context.github_ref}"
    )
    require(
        context.workflow_ref == expected_workflow_ref,
        "release publish authority workflow-ref mismatch: "
        f"expected {expected_workflow_ref} got {context.workflow_ref}",
    )
    require(
        context.github_job == policy["publish_job_id"],
        "release publish authority job mismatch: "
        f"expected {policy['publish_job_id']} got {context.github_job}",
    )
    require(
        context.token_source == policy["token_source"],
        "release publish authority token-source mismatch: "
        f"expected {policy['token_source']} got {context.token_source}",
    )
    require(
        os.environ.get(context.token_env, "").strip() != "",
        f"release publish authority token env {context.token_env} missing",
    )


def verify_workflow_policy(path: Path) -> None:
    require(path.is_file(), f"release publish authority workflow file missing: {path}")
    lines = path.read_text().splitlines()
    policy = PUBLISH_AUTHORITY_POLICY
    errors: list[str] = []

    observed_top_permissions = top_level_permissions(lines)
    if observed_top_permissions != policy["top_level_permissions"]:
        errors.append(
            f"{path}: top-level permissions must be "
            f"{policy['top_level_permissions']}, got {observed_top_permissions}"
        )

    jobs = {job_id: (start, end) for job_id, start, end in job_blocks(lines)}
    for required_job in [policy["build_job_id"], policy["publish_job_id"]]:
        if required_job not in jobs:
            errors.append(f"{path}: required release job missing: {required_job}")

    build_job = policy["build_job_id"]
    if build_job in jobs:
        start, end = jobs[build_job]
        observed = job_permissions(lines[start:end])
        if observed != policy["build_job_permissions"]:
            errors.append(
                f"{path}:{start + 1}: job {build_job} permissions must be "
                f"{policy['build_job_permissions']}, got {observed}"
            )

    publish_job = policy["publish_job_id"]
    if publish_job in jobs:
        start, end = jobs[publish_job]
        block = lines[start:end]
        observed = job_permissions(block)
        if observed != policy["publish_job_permissions"]:
            errors.append(
                f"{path}:{start + 1}: job {publish_job} permissions must be "
                f"{policy['publish_job_permissions']}, got {observed}"
            )
        if not job_has_tag_guard(block):
            errors.append(
                f"{path}:{start + 1}: job {publish_job} must be guarded by refs/tags/"
            )

    for job_id, start, end in job_blocks(lines):
        permissions = job_permissions(lines[start:end])
        if permissions is None:
            errors.append(f"{path}:{start + 1}: release job {job_id} must declare permissions")
            continue
        for scope, level in permissions.items():
            if not is_write_permission(level):
                continue
            if job_id == policy["publish_job_id"] and scope == "contents":
                continue
            if job_id == policy["build_job_id"] and scope in {"id-token", "attestations"}:
                continue
            errors.append(
                f"{path}:{start + 1}: unexpected write authority: "
                f"job {job_id} grants {scope}: {level}"
            )

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        raise SystemExit(1)


def top_level_permissions(lines: list[str]) -> dict[str, str] | None:
    for index, line in enumerate(lines):
        if line != "permissions:":
            continue
        suffix = line.split(":", 1)[1].strip()
        if suffix:
            return {"*": suffix}
        permissions: dict[str, str] = {}
        for nested in lines[index + 1 :]:
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if not nested.startswith("  "):
                break
            key_value = nested.strip().split(":", 1)
            if len(key_value) != 2:
                continue
            permissions[key_value[0]] = key_value[1].strip()
        return permissions
    return None


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
        if match is None:
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
            if match is not None:
                permissions[match.group(1)] = match.group(2)
        return permissions
    return None


def job_has_tag_guard(lines: list[str]) -> bool:
    return any(
        "startsWith(github.ref, 'refs/tags/')" in line
        or 'startsWith(github.ref, "refs/tags/")' in line
        for line in lines
    )


def is_write_permission(level: str) -> bool:
    return level in {"write", "write-all"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


if __name__ == "__main__":
    raise SystemExit(main())
