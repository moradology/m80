#!/usr/bin/env python3
"""Verify release publish authority before mutating GitHub release state."""

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
import sys


SCHEMA_VERSION = 1
KIND = "m80_release_publish_token_authority"
RECEIPT_NAME = "m80-release-token-authority.json"
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
    parser.add_argument("--release-tag", default=None)
    parser.add_argument("--commit-sha", default=None)
    parser.add_argument("--workflow-run-id", default=None)
    parser.add_argument("--workflow-run-attempt", default=None)
    parser.add_argument("--actor", default=None)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--generated-at")
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    receipt_path = args.receipt.resolve() if args.receipt else None
    receipt: dict | None = None
    try:
        context = PublishContext(
            repository=resolve_value(args.repository, "GITHUB_REPOSITORY", "repository"),
            github_ref=resolve_value(args.github_ref, "GITHUB_REF", "github-ref"),
            workflow_ref=resolve_value(args.workflow_ref, "GITHUB_WORKFLOW_REF", "github-workflow-ref"),
            github_job=resolve_value(args.github_job, "GITHUB_JOB", "github-job"),
            token_source=resolve_value(args.token_source, "M80_RELEASE_TOKEN_SOURCE", "token-source"),
            token_env=args.token_env,
        )
        verify_context(context)
        release_tag = resolve_release_tag(args.release_tag, context.github_ref)
        receipt = build_receipt(
            context,
            workflow_file=args.workflow_file,
            release_tag=release_tag,
            commit_sha=resolve_value(args.commit_sha, "RELEASE_COMMIT", "commit-sha"),
            workflow_run_id=resolve_optional(args.workflow_run_id, "GITHUB_RUN_ID"),
            workflow_run_attempt=resolve_optional(args.workflow_run_attempt, "GITHUB_RUN_ATTEMPT"),
            actor=resolve_optional(args.actor, "GITHUB_ACTOR"),
            generated_at=args.generated_at,
        )
        verify_workflow_policy(args.workflow_file)
        receipt["probes"].append(run_release_metadata_probe(context.repository, release_tag))
        verify_release_metadata_probe(receipt["probes"][-1], release_tag)
        receipt["decision"] = "approved"
        receipt["failure_reason"] = None
        verify_receipt(receipt, context, release_tag, resolve_value(args.commit_sha, "RELEASE_COMMIT", "commit-sha"))
        if args.write:
            require(receipt_path is not None, "release publish authority receipt path missing")
            write_json(receipt_path, receipt)
        elif receipt_path is not None:
            existing = read_json(receipt_path, "release publish authority receipt")
            verify_receipt(existing, context, release_tag, resolve_value(args.commit_sha, "RELEASE_COMMIT", "commit-sha"))
        print(
            "release publish authority ok: "
            f"{context.repository} {context.github_ref} {context.github_job}"
        )
        return 0
    except SystemExit as exc:
        message = str(exc)
        if args.write and receipt_path is not None:
            if receipt is not None:
                receipt["decision"] = "failed"
                receipt["failure_reason"] = message
                write_json(receipt_path, receipt)
            else:
                write_json(receipt_path, failure_receipt(args, message))
        raise SystemExit(message) from exc


def resolve_value(value: str | None, env_name: str, label: str) -> str:
    observed = value if value is not None else os.environ.get(env_name, "")
    require(observed.strip() != "", f"release publish authority {label} missing")
    return observed


def resolve_optional(value: str | None, env_name: str) -> str | None:
    observed = value if value is not None else os.environ.get(env_name)
    if observed is None or observed.strip() == "":
        return None
    return observed


def resolve_release_tag(value: str | None, github_ref: str) -> str:
    observed = value.strip() if value else ""
    if observed:
        return observed
    prefix = "refs/tags/"
    require(github_ref.startswith(prefix), "release publish authority release-tag missing")
    return github_ref[len(prefix):]


def build_receipt(
    context: PublishContext,
    *,
    workflow_file: Path,
    release_tag: str,
    commit_sha: str,
    workflow_run_id: str | None,
    workflow_run_attempt: str | None,
    actor: str | None,
    generated_at: str | None,
) -> dict:
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "decision": "failed",
        "repository": context.repository,
        "github_ref": context.github_ref,
        "release_tag": release_tag,
        "commit_sha": commit_sha,
        "workflow_ref": context.workflow_ref,
        "workflow_path": PUBLISH_AUTHORITY_POLICY["workflow_path"],
        "workflow_file": str(workflow_file),
        "github_job": context.github_job,
        "workflow_run_id": workflow_run_id,
        "workflow_run_attempt": workflow_run_attempt,
        "actor": actor,
        "token_source": context.token_source,
        "token_env": context.token_env,
        "policy_id": "m80-release-publish-authority-v1",
        "policy_digest": policy_digest(),
        "generated_at": generated_at or utc_now(),
        "probes": [],
        "failure_reason": "not evaluated",
    }


def failure_receipt(args: argparse.Namespace, message: str) -> dict:
    repository = args.repository or os.environ.get("GITHUB_REPOSITORY")
    github_ref = args.github_ref or os.environ.get("GITHUB_REF")
    workflow_ref = args.workflow_ref or os.environ.get("GITHUB_WORKFLOW_REF")
    github_job = args.github_job or os.environ.get("GITHUB_JOB")
    token_source = args.token_source or os.environ.get("M80_RELEASE_TOKEN_SOURCE")
    release_tag = args.release_tag or tag_from_ref(github_ref)
    commit_sha = args.commit_sha or os.environ.get("RELEASE_COMMIT")
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "decision": "failed",
        "repository": repository,
        "github_ref": github_ref,
        "release_tag": release_tag,
        "commit_sha": commit_sha,
        "workflow_ref": workflow_ref,
        "workflow_path": PUBLISH_AUTHORITY_POLICY["workflow_path"],
        "workflow_file": str(args.workflow_file),
        "github_job": github_job,
        "workflow_run_id": args.workflow_run_id or os.environ.get("GITHUB_RUN_ID"),
        "workflow_run_attempt": args.workflow_run_attempt or os.environ.get("GITHUB_RUN_ATTEMPT"),
        "actor": args.actor or os.environ.get("GITHUB_ACTOR"),
        "token_source": token_source,
        "token_env": args.token_env,
        "policy_id": "m80-release-publish-authority-v1",
        "policy_digest": policy_digest(),
        "generated_at": args.generated_at or utc_now(),
        "probes": [],
        "failure_reason": message,
    }


def tag_from_ref(github_ref: str | None) -> str | None:
    prefix = "refs/tags/"
    if github_ref and github_ref.startswith(prefix):
        return github_ref[len(prefix):]
    return None


def run_release_metadata_probe(repository: str, release_tag: str) -> dict:
    command = [
        "gh",
        "release",
        "view",
        release_tag,
        "--repo",
        repository,
        "--json",
        "tagName,url,isDraft,isPrerelease",
    ]
    result = subprocess.run(command, text=True, capture_output=True, check=False)
    probe = {
        "name": "release_metadata",
        "command": " ".join(command),
        "exit_status": result.returncode,
        "stdout": result.stdout.strip(),
        "stderr": result.stderr.strip(),
    }
    return probe


def verify_release_metadata_probe(probe: dict, release_tag: str) -> None:
    if probe["exit_status"] != 0:
        require(
            release_metadata_absent(probe),
            "release publish authority release metadata unreadable",
        )
        probe["release_state"] = "absent"
        return
    try:
        payload = json.loads(probe["stdout"])
    except json.JSONDecodeError as exc:
        raise SystemExit(f"release publish authority release metadata malformed: {exc}") from exc
    require(payload.get("tagName") == release_tag, "release publish authority release metadata tag mismatch")
    require(payload.get("isDraft") is False, "release publish authority release metadata is draft")
    require(payload.get("isPrerelease") is False, "release publish authority release metadata is prerelease")
    probe["release_state"] = "published"


def release_metadata_absent(probe: dict) -> bool:
    stderr = str(probe.get("stderr", "")).lower()
    return "release not found" in stderr


def verify_receipt(receipt: dict, context: PublishContext, release_tag: str, commit_sha: str) -> None:
    expected_keys = {
        "schema_version",
        "kind",
        "decision",
        "repository",
        "github_ref",
        "release_tag",
        "commit_sha",
        "workflow_ref",
        "workflow_path",
        "workflow_file",
        "github_job",
        "workflow_run_id",
        "workflow_run_attempt",
        "actor",
        "token_source",
        "token_env",
        "policy_id",
        "policy_digest",
        "generated_at",
        "probes",
        "failure_reason",
    }
    unknown = sorted(set(receipt) - expected_keys)
    require(not unknown, f"release publish authority receipt unknown fields: {', '.join(unknown)}")
    missing = sorted(expected_keys - set(receipt))
    require(not missing, f"release publish authority receipt missing fields: {', '.join(missing)}")
    require(receipt["schema_version"] == SCHEMA_VERSION, "release publish authority receipt schema_version mismatch")
    require(receipt["kind"] == KIND, "release publish authority receipt kind mismatch")
    require(receipt["decision"] == "approved", "release publish authority receipt decision must be approved")
    require(receipt["repository"] == context.repository, "release publish authority receipt repository mismatch")
    require(receipt["github_ref"] == context.github_ref, "release publish authority receipt github_ref mismatch")
    require(receipt["release_tag"] == release_tag, "release publish authority receipt release_tag mismatch")
    require(receipt["commit_sha"] == commit_sha, "release publish authority receipt commit_sha mismatch")
    require(receipt["workflow_ref"] == context.workflow_ref, "release publish authority receipt workflow_ref mismatch")
    require(receipt["github_job"] == context.github_job, "release publish authority receipt github_job mismatch")
    require(receipt["token_source"] == context.token_source, "release publish authority receipt token_source mismatch")
    require(receipt["token_env"] == context.token_env, "release publish authority receipt token_env mismatch")
    require(receipt["policy_digest"] == policy_digest(), "release publish authority receipt policy_digest mismatch")
    require(receipt["failure_reason"] is None, "release publish authority receipt approved with failure_reason")
    require(isinstance(receipt["probes"], list) and receipt["probes"], "release publish authority receipt probes missing")
    require(parse_timestamp(receipt["generated_at"]) is not None, "release publish authority receipt generated_at invalid")


def policy_digest() -> str:
    data = json.dumps(PUBLISH_AUTHORITY_POLICY, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(data).hexdigest()


def parse_timestamp(value: object) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path}")
    try:
        value = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{label} malformed: {exc}") from exc
    require(isinstance(value, dict), f"{label} must be a JSON object")
    return value


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
        raise SystemExit("; ".join(errors))


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
