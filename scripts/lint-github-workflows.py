#!/usr/bin/env python3
"""Fail closed on GitHub workflow authority-policy drift."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys


FULL_SHA_RE = re.compile(r"[0-9a-fA-F]{40}")
USES_RE = re.compile(r"^\s*(?:-\s*)?uses:\s*([^#\s]+)")
JOB_RE = re.compile(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$")
PERMISSION_RE = re.compile(r"^\s{6}([A-Za-z0-9_-]+):\s*([A-Za-z0-9_-]+)\s*(?:#.*)?$")
SECRET_RE = re.compile(r"\$\{\{\s*secrets\.")
CARGO_COMMAND_RE = re.compile(r"(?:^|\s)cargo(?:\s+\+\S+)?\s+(build|test|clippy|install)\b")
EXPENSIVE_RUST_CI_RE = re.compile(
    r"(?:^|\s)(?:cargo\s+(?:fmt|build|test|clippy)\b|rustup\s+toolchain\s+install\b)"
)
RUSTUP_TOOLCHAIN_INSTALL_RE = re.compile(r"(?:^|\s)rustup\s+toolchain\s+install\s+(\S+)")
RUSTUP_TARGET_ADD_RE = re.compile(r"(?:^|\s)rustup\s+target\s+add\b")
PINNED_RUST_TOOLCHAIN_RE = re.compile(r"^[0-9]+\.[0-9]+(?:\.[0-9]+)?$")
MULTILINE_RUN_RE = re.compile(r"^(\s*)(?:-\s*)?run\s*:\s*[|>][+-]?")
COMMAND_SUBSTITUTION_RE = re.compile(r"\$\(")
ASSIGNMENT_COMMAND_SUBSTITUTION_RE = re.compile(
    r"^[A-Za-z_][A-Za-z0-9_]*=(?:\"|')?\$\("
)
RUN_BLOCK_STRICT_PREAMBLE = "set -euo pipefail"
RUN_BLOCK_EXCEPTION_MARKER = "m80-lint: allow-nonstrict-run"
MAX_RELEASE_JOB_TIMEOUT_MINUTES = 120
RELEASE_PUBLISH_ENVIRONMENT = "m80-release-publish"
REUSABLE_TIMEOUT_MARKER_RE = re.compile(
    r"m80-lint:\s*reusable-timeout-minutes=([0-9]+)\b"
)
WORKFLOW_POLICY_CONFIG = Path("docs/behaviors/ci/workflow-policy-scope.json")
WORKFLOW_TIMEOUT_BUDGET_CONFIG = Path("docs/behaviors/ci/workflow-timeout-budgets.json")
RELEASE_RUNBOOK = Path("docs/runbook/release.md")
ALLOWED_WORKFLOW_SCOPES = {
    "ordinary-ci",
    "release-authority",
    "latest-freshness",
    "proof",
}
RELEASE_GUARD_SCOPES = {"release-authority", "latest-freshness", "proof"}
GUARDED_FILENAME_TOKENS = ["release", "latest", "freshness", "proof", "publish"]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--workflow-dir",
        type=Path,
        default=Path(".github/workflows"),
        help="directory containing GitHub workflow YAML files",
    )
    parser.add_argument(
        "--policy-config",
        type=Path,
        default=WORKFLOW_POLICY_CONFIG,
        help="JSON config that assigns an explicit scope to every workflow file",
    )
    parser.add_argument(
        "--timeout-budget-config",
        type=Path,
        default=WORKFLOW_TIMEOUT_BUDGET_CONFIG,
        help="JSON config that assigns exact timeout budgets to workflow jobs",
    )
    parser.add_argument(
        "--runbook",
        type=Path,
        default=RELEASE_RUNBOOK,
        help="release runbook whose timeout table is checked when present",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    errors = lint_workflow_dir(
        args.workflow_dir,
        policy_config=args.policy_config,
        timeout_budget_config=args.timeout_budget_config,
        runbook=args.runbook,
    )
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"workflow policy ok: {args.workflow_dir}")
    return 0


def lint_workflow_dir(
    workflow_dir: Path,
    *,
    policy_config: Path = WORKFLOW_POLICY_CONFIG,
    timeout_budget_config: Path = WORKFLOW_TIMEOUT_BUDGET_CONFIG,
    runbook: Path = RELEASE_RUNBOOK,
) -> list[str]:
    files = sorted(set(workflow_dir.glob("*.yml")) | set(workflow_dir.glob("*.yaml")))
    if not files:
        return [f"{workflow_dir}: no workflow files found"]
    scope_by_name, errors = load_workflow_scope_policy(
        policy_config=policy_config,
        workflow_dir=workflow_dir,
        files=files,
    )
    timeout_budgets, timeout_errors = load_timeout_budget_policy(
        timeout_budget_config=timeout_budget_config,
        workflow_dir=workflow_dir,
        files=files,
    )
    errors.extend(timeout_errors)
    errors.extend(lint_timeout_budget_runbook(runbook, timeout_budgets))
    seen_jobs: set[tuple[str, str]] = set()
    for path in files:
        text = path.read_text()
        lines = text.splitlines()
        workflow_scope = scope_by_name.get(path.name)
        release_workflow = workflow_scope in RELEASE_GUARD_SCOPES
        errors.extend(lint_action_refs(path, lines))
        errors.extend(lint_top_level_permissions(path, lines))
        errors.extend(lint_job_permissions(path, lines, release_workflow=release_workflow))
        errors.extend(lint_attestation_permissions(path, lines, release_workflow=release_workflow))
        errors.extend(lint_multiline_run_block_strictness(path, lines))
        if path.name == "ci.yml":
            errors.extend(lint_ci_changed_line_whitespace_check(path, lines))
        if workflow_scope == "latest-freshness":
            errors.extend(lint_freshness_workflow(path, text, lines))
        if release_workflow:
            errors.extend(lint_release_concurrency(path, lines))
            errors.extend(lint_release_job_timeouts(path, lines, timeout_budgets=timeout_budgets))
            errors.extend(lint_release_cargo_locked(path, lines))
            errors.extend(lint_release_rust_toolchain_pins(path, lines))
            errors.extend(lint_release_artifact_origin(path, text, lines))
            errors.extend(lint_release_publish_environment(path, lines))
            errors.extend(lint_release_mutation_authority(path, lines))
            errors.extend(lint_release_temp_isolation(path, text, lines))
        if has_event(lines, "pull_request") and SECRET_RE.search(text):
            errors.append(f"{path}: pull_request workflow must not reference secrets.*")
        errors.extend(lint_configured_timeout_jobs(path, lines, timeout_budgets, seen_jobs))
    for workflow_name, job_id in sorted(set(timeout_budgets) - seen_jobs):
        errors.append(
            f"{timeout_budget_config}: configured timeout job is missing from workflows: "
            f"{workflow_name}:{job_id}"
        )
    return errors


def load_workflow_scope_policy(
    *,
    policy_config: Path,
    workflow_dir: Path,
    files: list[Path],
) -> tuple[dict[str, str], list[str]]:
    errors: list[str] = []
    if not policy_config.exists():
        return {}, [f"{policy_config}: workflow scope policy config is missing"]

    try:
        payload = json.loads(policy_config.read_text())
    except json.JSONDecodeError as exc:
        return {}, [f"{policy_config}:{exc.lineno}: invalid JSON: {exc.msg}"]

    if not isinstance(payload, dict):
        return {}, [f"{policy_config}: workflow scope policy must be a JSON object"]
    allowed_keys = {"schema_version", "workflows"}
    unknown_keys = sorted(set(payload) - allowed_keys)
    for key in unknown_keys:
        errors.append(f"{policy_config}: unknown workflow scope policy field: {key}")
    if payload.get("schema_version") != 1:
        errors.append(f"{policy_config}: schema_version must be 1")

    entries = payload.get("workflows")
    if not isinstance(entries, list):
        errors.append(f"{policy_config}: workflows must be a list")
        return {}, errors

    names_on_disk = {path.name for path in files}
    scope_by_name: dict[str, str] = {}
    for index, entry in enumerate(entries):
        prefix = f"{policy_config}:workflows[{index}]"
        if not isinstance(entry, dict):
            errors.append(f"{prefix}: workflow scope entry must be an object")
            continue
        unknown_entry_keys = sorted(set(entry) - {"path", "scope"})
        for key in unknown_entry_keys:
            errors.append(f"{prefix}: unknown workflow scope entry field: {key}")
        workflow_path = entry.get("path")
        scope = entry.get("scope")
        if not isinstance(workflow_path, str) or not workflow_path:
            errors.append(f"{prefix}: path must be a non-empty string")
            continue
        if "/" in workflow_path or workflow_path in {".", ".."} or workflow_path.startswith("."):
            errors.append(f"{prefix}: path must be a workflow filename under {workflow_dir}")
            continue
        if workflow_path in scope_by_name:
            errors.append(f"{prefix}: duplicate workflow scope entry for {workflow_path}")
            continue
        if workflow_path not in names_on_disk:
            errors.append(f"{policy_config}: configured workflow is missing: {workflow_path}")
        if scope not in ALLOWED_WORKFLOW_SCOPES:
            errors.append(f"{prefix}: unknown workflow scope: {scope}")
            continue
        scope_by_name[workflow_path] = scope

    for path in files:
        if path.name in scope_by_name:
            continue
        if workflow_needs_release_guards(path):
            errors.append(
                f"{path}: guarded-looking workflow filename is not named in {policy_config}"
            )
        else:
            errors.append(f"{path}: workflow is not named in {policy_config}")
    return scope_by_name, errors


def load_timeout_budget_policy(
    *,
    timeout_budget_config: Path,
    workflow_dir: Path,
    files: list[Path],
) -> tuple[dict[tuple[str, str], int], list[str]]:
    errors: list[str] = []
    if not timeout_budget_config.exists():
        return {}, [f"{timeout_budget_config}: workflow timeout budget config is missing"]

    try:
        payload = json.loads(timeout_budget_config.read_text())
    except json.JSONDecodeError as exc:
        return {}, [f"{timeout_budget_config}:{exc.lineno}: invalid JSON: {exc.msg}"]

    if not isinstance(payload, dict):
        return {}, [f"{timeout_budget_config}: timeout budget policy must be a JSON object"]
    allowed_keys = {"schema_version", "jobs"}
    for key in sorted(set(payload) - allowed_keys):
        errors.append(f"{timeout_budget_config}: unknown timeout budget policy field: {key}")
    if payload.get("schema_version") != 1:
        errors.append(f"{timeout_budget_config}: schema_version must be 1")

    entries = payload.get("jobs")
    if not isinstance(entries, list):
        errors.append(f"{timeout_budget_config}: jobs must be a list")
        return {}, errors

    names_on_disk = {path.name for path in files}
    budgets: dict[tuple[str, str], int] = {}
    for index, entry in enumerate(entries):
        prefix = f"{timeout_budget_config}:jobs[{index}]"
        if not isinstance(entry, dict):
            errors.append(f"{prefix}: timeout budget entry must be an object")
            continue
        for key in sorted(set(entry) - {"path", "job", "timeout_minutes"}):
            errors.append(f"{prefix}: unknown timeout budget entry field: {key}")
        workflow_path = entry.get("path")
        job_id = entry.get("job")
        timeout_minutes = entry.get("timeout_minutes")
        if not isinstance(workflow_path, str) or not workflow_path:
            errors.append(f"{prefix}: path must be a non-empty string")
            continue
        if "/" in workflow_path or workflow_path in {".", ".."} or workflow_path.startswith("."):
            errors.append(f"{prefix}: path must be a workflow filename under {workflow_dir}")
            continue
        if workflow_path not in names_on_disk:
            errors.append(f"{timeout_budget_config}: configured workflow is missing: {workflow_path}")
        if not isinstance(job_id, str) or not job_id:
            errors.append(f"{prefix}: job must be a non-empty string")
            continue
        if not isinstance(timeout_minutes, int) or isinstance(timeout_minutes, bool):
            errors.append(f"{prefix}: timeout_minutes must be an integer")
            continue
        key = (workflow_path, job_id)
        if key in budgets:
            errors.append(f"{prefix}: duplicate timeout budget entry for {workflow_path}:{job_id}")
            continue
        errors.extend(
            lint_timeout_budget(
                timeout_budget_config,
                line_no=index + 1,
                subject=f"configured timeout budget {workflow_path}:{job_id}",
                minutes=timeout_minutes,
            )
        )
        budgets[key] = timeout_minutes
    return budgets, errors


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
                errors.extend(
                    publish_write_authority_errors(
                        path,
                        job_id=job_id,
                        line_no=start + 1,
                        block=lines[start:end],
                    )
                )
                continue
            if release_workflow and is_attestation_build_permission(job_id, scope):
                continue
            errors.append(
                f"{path}:{start + 1}: job {job_id} must not grant {scope}: {level}"
            )
    return errors


def publish_write_authority_errors(
    path: Path,
    *,
    job_id: str,
    line_no: int,
    block: list[str],
) -> list[str]:
    block_text = "\n".join(block)
    if path.name == "release-artifacts.yml":
        required = "needs: build-release-artifacts"
        if required in block_text:
            return []
        return [
            f"{path}:{line_no}: job {job_id} grants contents: write but policy "
            "requires needs: build-release-artifacts before release mutation"
        ]
    if path.name == "latest-freshness.yml":
        required = [
            "needs: hostless-public-freshness",
            "needs.hostless-public-freshness.result == 'success'",
        ]
        missing = [token for token in required if token not in block_text]
        if not missing:
            return []
        return [
            f"{path}:{line_no}: job {job_id} grants contents: write but policy "
            f"requires freshness readiness needs before release mutation: {', '.join(missing)}"
        ]
    return [
        f"{path}:{line_no}: job {job_id} grants contents: write but no "
        "release mutation authority policy names its required needs graph"
    ]


def lint_attestation_permissions(
    path: Path,
    lines: list[str],
    *,
    release_workflow: bool,
) -> list[str]:
    if not release_workflow:
        return []
    errors: list[str] = []
    for job_id, start, end in job_blocks(lines):
        block = lines[start:end]
        if not any("uses: actions/attest@" in line for line in block):
            continue
        permissions = job_permissions(block)
        for scope in ["id-token", "attestations"]:
            if permissions is None or permissions.get(scope) != "write":
                errors.append(
                    f"{path}:{start + 1}: job {job_id} uses actions/attest and must grant {scope}: write"
                )
        if not is_attestation_build_job(job_id):
            errors.append(
                f"{path}:{start + 1}: job {job_id} must not generate release attestations"
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


def lint_release_cargo_locked(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for line_no, line in enumerate(lines, start=1):
        if not CARGO_COMMAND_RE.search(line):
            continue
        if "--locked" not in line:
            errors.append(f"{path}:{line_no}: release cargo invocation must use --locked")
    return errors


def lint_release_rust_toolchain_pins(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for line_no, line in enumerate(lines, start=1):
        install = RUSTUP_TOOLCHAIN_INSTALL_RE.search(line)
        if install and PINNED_RUST_TOOLCHAIN_RE.fullmatch(install.group(1)) is None:
            errors.append(f"{path}:{line_no}: release rustup toolchain install must use a pinned numeric toolchain")
        if RUSTUP_TARGET_ADD_RE.search(line):
            toolchain = rustup_target_toolchain(line)
            if toolchain is None or PINNED_RUST_TOOLCHAIN_RE.fullmatch(toolchain) is None:
                errors.append(f"{path}:{line_no}: release rustup target add must use --toolchain with a pinned numeric toolchain")
    return errors


def lint_release_job_timeouts(
    path: Path,
    lines: list[str],
    *,
    timeout_budgets: dict[tuple[str, str], int],
) -> list[str]:
    errors: list[str] = []
    for job_id, start, end in job_blocks(lines):
        block = lines[start:end]
        budget = timeout_budgets.get((path.name, job_id))
        if budget is None:
            errors.append(f"{path}:{start + 1}: release job {job_id} missing timeout budget config entry")
        found_timeout = False
        valid_timeouts: list[tuple[int, int]] = []
        for offset, line in enumerate(block):
            if not line.startswith("    timeout-minutes:"):
                continue
            found_timeout = True
            value = line.split(":", 1)[1].split("#", 1)[0].strip()
            if not value.isdigit():
                errors.append(
                    f"{path}:{start + offset + 1}: release job {job_id} timeout-minutes "
                    "must be an integer minute value"
                )
                continue
            valid_timeouts.append((int(value), offset))

        if len(valid_timeouts) > 1:
            errors.append(
                f"{path}:{start + 1}: release job {job_id} must declare only one timeout-minutes"
            )
        if valid_timeouts:
            minutes, offset = valid_timeouts[0]
            errors.extend(
                lint_timeout_budget(
                    path,
                    line_no=start + offset + 1,
                    subject=f"release job {job_id}",
                    minutes=minutes,
                )
            )
            if budget is not None and minutes != budget:
                errors.append(
                    f"{path}:{start + offset + 1}: release job {job_id} timeout-minutes "
                    f"{minutes} does not match configured budget {budget}"
                )
            continue
        if found_timeout:
            continue

        if job_uses_reusable_workflow(block):
            marker = reusable_timeout_marker_minutes(path, job_id, start, block)
            errors.extend(marker[1])
            if marker[0] is None:
                errors.append(
                    f"{path}:{start + 1}: release reusable job {job_id} must declare "
                    "timeout-minutes or document m80-lint: reusable-timeout-minutes=N"
                )
            else:
                errors.extend(
                    lint_timeout_budget(
                        path,
                        line_no=marker[2],
                        subject=f"release reusable job {job_id}",
                        minutes=marker[0],
                    )
                )
                if budget is not None and marker[0] != budget:
                    errors.append(
                        f"{path}:{marker[2]}: release reusable job {job_id} timeout marker "
                        f"{marker[0]} does not match configured budget {budget}"
                    )
            continue

        errors.append(f"{path}:{start + 1}: release job {job_id} must declare timeout-minutes")
    return errors


def lint_configured_timeout_jobs(
    path: Path,
    lines: list[str],
    timeout_budgets: dict[tuple[str, str], int],
    seen_jobs: set[tuple[str, str]],
) -> list[str]:
    errors: list[str] = []
    for job_id, start, end in job_blocks(lines):
        key = (path.name, job_id)
        budget = timeout_budgets.get(key)
        if budget is None:
            continue
        seen_jobs.add(key)
        block = lines[start:end]
        if job_uses_reusable_workflow(block):
            marker = reusable_timeout_marker_minutes(path, job_id, start, block)
            if marker[0] is None:
                continue
            if marker[0] != budget:
                errors.append(
                    f"{path}:{marker[2]}: configured job {job_id} reusable timeout marker "
                    f"{marker[0]} does not match configured budget {budget}"
                )
            continue
        timeout = job_timeout_minutes(block)
        if timeout is None:
            continue
        minutes, line_no = timeout
        if minutes != budget:
            errors.append(
                f"{path}:{start + line_no}: configured job {job_id} timeout-minutes "
                f"{minutes} does not match configured budget {budget}"
            )
    return errors


def lint_ci_changed_line_whitespace_check(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    workflow_text = "\n".join(lines)
    if (
        "fetch-depth: 0" not in workflow_text
        and "git diff --check" not in workflow_text
        and not (has_event(lines, "push") and has_event(lines, "pull_request"))
    ):
        return []
    if not has_event(lines, "push"):
        errors.append(f"{path}: CI workflow must run the whitespace check on push")
    if not has_event(lines, "pull_request"):
        errors.append(f"{path}: CI workflow must run the whitespace check on pull_request")

    diff_check_line = first_line_index_containing(lines, "git diff --check")
    if diff_check_line is None:
        return errors + [f"{path}: CI workflow must run git diff --check before Rust steps"]

    text_before_diff = "\n".join(lines[:diff_check_line + 1])
    if "github.event.before" not in text_before_diff:
        errors.append(f"{path}:{diff_check_line + 1}: git diff --check must use github.event.before for push")
    if "github.event.pull_request.base.sha" not in text_before_diff:
        errors.append(
            f"{path}:{diff_check_line + 1}: git diff --check must use pull_request.base.sha for PRs"
        )
    if 'git cat-file -e "$BASE_SHA^{commit}"' not in text_before_diff:
        errors.append(
            f"{path}:{diff_check_line + 1}: git diff --check must prove the base commit exists"
        )
    if '"0000000000000000000000000000000000000000"' not in text_before_diff:
        errors.append(
            f"{path}:{diff_check_line + 1}: git diff --check must handle branch-creation push events"
        )

    first_expensive_rust_line = first_expensive_rust_ci_line(lines)
    if first_expensive_rust_line is not None and diff_check_line > first_expensive_rust_line:
        errors.append(
            f"{path}:{diff_check_line + 1}: git diff --check must run before expensive Rust steps"
        )
    return errors


def first_line_index_containing(lines: list[str], needle: str) -> int | None:
    for index, line in enumerate(lines):
        if needle in line:
            return index
    return None


def first_expensive_rust_ci_line(lines: list[str]) -> int | None:
    for index, line in enumerate(lines):
        if EXPENSIVE_RUST_CI_RE.search(line):
            return index
    return None


def job_timeout_minutes(lines: list[str]) -> tuple[int, int] | None:
    for offset, line in enumerate(lines, start=1):
        if not line.startswith("    timeout-minutes:"):
            continue
        value = line.split(":", 1)[1].split("#", 1)[0].strip()
        if value.isdigit():
            return int(value), offset
        return None
    return None


def lint_timeout_budget_runbook(
    runbook: Path,
    timeout_budgets: dict[tuple[str, str], int],
) -> list[str]:
    if not timeout_budgets or not runbook.exists():
        return []
    text = runbook.read_text()
    actual = extract_timeout_budget_table(text)
    expected = expected_timeout_budget_table(timeout_budgets)
    if actual == expected:
        return []
    return [
        f"{runbook}: workflow timeout budget table is stale; expected:\n"
        + "\n".join(expected)
    ]


def extract_timeout_budget_table(text: str) -> list[str]:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != "| Workflow | Job | Timeout |":
            continue
        table: list[str] = []
        for current in lines[index:]:
            if not current.startswith("|"):
                break
            table.append(current.rstrip())
        return table
    return []


def expected_timeout_budget_table(timeout_budgets: dict[tuple[str, str], int]) -> list[str]:
    rows = [
        "| Workflow | Job | Timeout |",
        "| --- | --- | --- |",
    ]
    for workflow_name, job_id in sorted(timeout_budgets):
        rows.append(
            f"| `.github/workflows/{workflow_name}` | `{job_id}` | "
            f"{timeout_budgets[(workflow_name, job_id)]} minutes |"
        )
    return rows


def lint_freshness_workflow(path: Path, text: str, lines: list[str]) -> list[str]:
    errors: list[str] = []
    if not has_schedule_event(lines):
        errors.append(f"{path}: freshness workflow must declare a schedule trigger")
    if not has_workflow_dispatch_event(lines):
        errors.append(f"{path}: freshness workflow must declare workflow_dispatch")
    if top_level_concurrency_cancel_in_progress(lines) != "false":
        errors.append(f"{path}: freshness workflow concurrency must set cancel-in-progress: false")
    if "scripts/release_freshness.py" not in text:
        errors.append(f"{path}: freshness workflow must invoke scripts/release_freshness.py")
    if not has_always_artifact_upload(lines):
        errors.append(f"{path}: freshness workflow must upload proof/log artifacts with if: always()")
    if not has_freshness_release_publish(lines):
        errors.append(
            f"{path}: freshness workflow must publish m80-latest-freshness-proof.json to the resolved latest release"
        )
    forbidden_mutations = [
        "gh release upload",
        "gh release edit",
        "gh release delete",
        "gh api --method POST",
        "gh api --method PATCH",
        "gh api --method DELETE",
    ]
    for job_id, start, end in job_blocks(lines):
        block = lines[start:end]
        block_text = "\n".join(block)
        for forbidden in forbidden_mutations:
            if forbidden not in block_text:
                continue
            if forbidden == "gh release upload" and is_publish_job(job_id):
                continue
            errors.append(f"{path}: freshness workflow must not run release mutation command: {forbidden}")
        for offset, line in enumerate(block):
            if "runs-on:" not in line:
                continue
            if any(token in line for token in ["self-hosted", "real-kvm", "privileged"]):
                errors.append(
                    f"{path}:{start + offset + 1}: freshness job {job_id} must stay hostless"
                )
    return errors


def lint_release_artifact_origin(path: Path, text: str, lines: list[str]) -> list[str]:
    if path.name != "release-artifacts.yml":
        return []
    if "build-release-artifacts:" not in text and "publish-release-artifacts:" not in text:
        return []
    errors: list[str] = []
    if workflow_dispatch_declares_inputs(lines):
        errors.append(f"{path}: release workflow must not accept manual artifact override inputs")
    if "actions/cache@" in text:
        errors.append(f"{path}: release workflow must not use cache contents as release artifact input")
    build_block = job_block_text(lines, "build-release-artifacts")
    publish_block = job_block_text(lines, "publish-release-artifacts")
    if build_block is None:
        errors.append(f"{path}: release workflow must define build-release-artifacts job")
        build_text = ""
    else:
        build_text = "\n".join(build_block)
    if publish_block is None:
        errors.append(f"{path}: release workflow must define publish-release-artifacts job")
        publish_text = ""
    else:
        publish_text = "\n".join(publish_block)

    required_build_tokens = {
        "release_commit: ${{ steps.release-commit.outputs.sha }}": "build job must output the source commit",
        "release_dist_artifact_id: ${{ steps.upload-release-dist.outputs.artifact-id }}": "build job must output release_dist_artifact_id from upload-release-dist",
        "release_dist_artifact_name: ${{ steps.release-artifact-origin.outputs.name }}": "build job must output release_dist_artifact_name from release-artifact-origin",
        "release_dist_producer_job: ${{ steps.release-artifact-origin.outputs.producer_job }}": "build job must output release_dist_producer_job from release-artifact-origin",
        "release_tag: ${{ steps.release-artifact-origin.outputs.release_tag }}": "build job must output release_tag from release-artifact-origin",
        "release_upload_manifest_digest: ${{ steps.release-upload-manifest.outputs.digest }}": "build job must output release_upload_manifest_digest from the build manifest step",
        "id: release-upload-manifest": "build job must give the release upload manifest step a stable id",
        "id: upload-release-dist": "build job must give the upload-artifact step a stable id",
        "id: release-artifact-origin": "build job must record release artifact origin with a stable id",
        "uses: actions/upload-artifact@": "build job must upload release bytes as a workflow artifact",
        "name: ${{ env.ARTIFACT_NAME }}": "build upload must use the fixed workflow artifact name",
        "path: ${{ steps.build-scratch.outputs.dist_dir }}/*": "build upload must use the verified release dist directory",
        "producer_job=build-release-artifacts": "build job must record the producing job id",
        "release_tag=$GITHUB_REF_NAME": "build job must record the source release tag",
    }
    for token, message in required_build_tokens.items():
        if token not in build_text:
            errors.append(f"{path}: {message}")

    required_publish_tokens = {
        "uses: actions/download-artifact@": "publish job must download release bytes from workflow artifacts",
        "artifact-ids: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_id }}": "publish download must use the recorded build artifact id",
        "merge-multiple: true": "publish download must flatten the selected artifact into the verified upload directory",
        "EXPECTED_ARTIFACT_ID: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_id }}": "publish job must carry the recorded build artifact id",
        "EXPECTED_ARTIFACT_NAME: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_name }}": "publish job must carry the recorded build artifact name",
        "EXPECTED_PRODUCER_JOB: ${{ needs.build-release-artifacts.outputs.release_dist_producer_job }}": "publish job must carry the recorded producer job id",
        "EXPECTED_RELEASE_TAG: ${{ needs.build-release-artifacts.outputs.release_tag }}": "publish job must carry the recorded release tag",
        "EXPECTED_MANIFEST_DIGEST: ${{ needs.build-release-artifacts.outputs.release_upload_manifest_digest }}": "publish job must carry the recorded build manifest digest",
        "test -n \"$EXPECTED_ARTIFACT_ID\"": "publish job must fail closed on missing build artifact id",
        "test \"$EXPECTED_ARTIFACT_NAME\" = \"$ARTIFACT_NAME\"": "publish job must check artifact name handoff",
        "test \"$EXPECTED_PRODUCER_JOB\" = \"build-release-artifacts\"": "publish job must check the producer job handoff",
        "test \"$EXPECTED_RELEASE_TAG\" = \"$GITHUB_REF_NAME\"": "publish job must check the release tag handoff",
        "test \"$observed_digest\" = \"$EXPECTED_MANIFEST_DIGEST\"": "publish job must hash-check the downloaded artifact set manifest",
        "scripts/release_upload_manifest.py": "publish job must size/hash-check the downloaded artifact set before upload",
        "--dist-dir \"$UPLOAD_DIR\"": "publish manifest check must inspect the downloaded release artifact set",
        "scripts/release_readiness_decision.py": "publish job must run the aggregate readiness decision before release mutation",
        "--stage pre-upload": "publish job must write the pre-upload readiness decision",
        "--stage pre-latest": "publish job must recheck hosted readiness before latest promotion",
        "--out \"$UPLOAD_DIR/m80-release-readiness-decision.json\"": "publish job must preserve the pre-upload readiness decision",
        "--readiness-decision \"$UPLOAD_DIR/m80-release-readiness-decision.json\"": "publish decision must bind the readiness decision",
        "name: m80-release-readiness-decision-${{ github.run_id }}": "publish job must upload the readiness decision artifact",
        "scripts/repository_protection_audit.py": "publish job must audit repository protections before release mutation",
        "--environment m80-release-publish": "repository protection audit must check the protected publish environment",
        "--out \"$UPLOAD_DIR/m80-repository-protection-audit.json\"": "repository protection audit must preserve a publish evidence artifact",
        "name: m80-repository-protection-audit-${{ github.run_id }}": "publish job must upload the repository protection audit artifact",
        "scripts/release_latest_promotion.py": "publish job must approve latest promotion before moving latest",
        "--release-list \"$REDOWNLOAD_DIR/github-releases-before-latest.json\"": "latest promotion decision must use a fresh public release list",
        "--publish-decision \"$UPLOAD_DIR/m80-release-publish-decision.json\"": "latest promotion decision must bind the publish decision receipt",
        "--proof-ledger \"$UPLOAD_DIR/m80-release-proof-ledger.jsonl\"": "latest promotion decision must bind the proof ledger",
        "--upload-manifest \"$UPLOAD_DIR/m80-release-upload-manifest.json\"": "latest promotion decision must bind the upload manifest",
        "--remote-inventory \"$REDOWNLOAD_DIR/m80-release-remote-assets.json\"": "latest promotion decision must bind verified remote asset state",
        "--out \"$UPLOAD_DIR/m80-latest-promotion-decision.json\"": "latest promotion decision must preserve an evidence artifact",
        "name: m80-latest-promotion-decision-${{ github.run_id }}": "publish job must upload the latest promotion decision artifact",
        "name: m80-latest-rollback-receipt-${{ github.run_id }}": "publish job must upload rollback receipt evidence when present",
        "path: docs/operations/release-latest-rollback-receipt.json": "rollback receipt upload must preserve the validated operator receipt",
    }
    for token, message in required_publish_tokens.items():
        if token not in publish_text:
            errors.append(f"{path}: {message}")

    for forbidden in [
        "M80_RELEASE_ARTIFACT_URL",
        "M80_RELEASE_DIST",
        "M80_RELEASE_ARTIFACT_PATH",
        "github.event.inputs.artifact",
        "github.event.inputs.dist",
        "github.event.inputs.url",
    ]:
        if forbidden in text:
            errors.append(f"{path}: release workflow must not accept artifact override {forbidden}")
    if "name: ${{ env.ARTIFACT_NAME }}" in publish_text:
        errors.append(f"{path}: publish job must not download by runner-local artifact env")
    if "name: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_name }}" in publish_text:
        errors.append(f"{path}: publish job must download by recorded artifact id, not name alone")
    if "DIST_DIR" in publish_text:
        errors.append(f"{path}: publish job must not reuse runner-local dist paths")
    if "steps.build-scratch.outputs.dist_dir" in publish_text:
        errors.append(f"{path}: publish job must not reuse runner-local dist paths")
    return errors


def lint_release_publish_environment(path: Path, lines: list[str]) -> list[str]:
    if path.name != "release-artifacts.yml":
        return []
    errors: list[str] = []
    for job_id, start, end in job_blocks(lines):
        environment = job_environment_name(lines[start:end])
        if job_id == "publish-release-artifacts":
            if environment != RELEASE_PUBLISH_ENVIRONMENT:
                errors.append(
                    f"{path}:{start + 1}: job {job_id} must target protected "
                    f"environment {RELEASE_PUBLISH_ENVIRONMENT}"
                )
            continue
        if environment == RELEASE_PUBLISH_ENVIRONMENT:
            errors.append(
                f"{path}:{start + 1}: job {job_id} must not target protected "
                f"environment {RELEASE_PUBLISH_ENVIRONMENT}"
            )
    return errors


def lint_release_mutation_authority(path: Path, lines: list[str]) -> list[str]:
    if path.name != "release-artifacts.yml":
        return []
    errors: list[str] = []
    forbidden_mutations = [
        "gh release create",
        "gh release upload",
        "gh release edit",
        "gh release delete",
        "gh api --method POST",
        "gh api --method PATCH",
        "gh api --method DELETE",
    ]
    for job_id, start, end in job_blocks(lines):
        if job_id == "publish-release-artifacts":
            continue
        block_text = "\n".join(lines[start:end])
        for forbidden in forbidden_mutations:
            if forbidden in block_text:
                errors.append(
                    f"{path}:{start + 1}: job {job_id} must not run release mutation "
                    f"command: {forbidden}"
                )
    return errors


def lint_release_temp_isolation(path: Path, text: str, lines: list[str]) -> list[str]:
    if path.name != "release-artifacts.yml":
        return []
    if "build-release-artifacts:" not in text and "publish-release-artifacts:" not in text:
        return []
    errors: list[str] = []
    if "/tmp/m80-release-" in text:
        errors.append(f"{path}: release workflow must not use fixed /tmp/m80-release-* scratch paths")

    required_by_job = {
        "build-release-artifacts": [
            "release_tmp_root=\"$RUNNER_TEMP/m80-release-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-build\"",
            "artifact_dir=\"$release_tmp_root/artifacts\"",
            "dist_dir=\"$release_tmp_root/dist\"",
            "test ! -e \"$release_tmp_root\"",
            "mkdir -p \"$artifact_dir\" \"$dist_dir\"",
            "test ! -L \"$release_tmp_root\"",
            "owner_uid=\"$(stat -c '%u' \"$release_tmp_root\")\"",
            "current_uid=\"$(id -u)\"",
            "test \"$owner_uid\" = \"$current_uid\"",
            "mode_octal=\"$(stat -c '%a' \"$release_tmp_root\")\"",
            "test \"$mode_octal\" = \"700\"",
            "echo \"RELEASE_TMP_ROOT=$release_tmp_root\"",
            "echo \"DIST_DIR=$dist_dir\"",
            "rm -rf \"$RELEASE_TMP_ROOT\"",
        ],
        "publish-release-artifacts": [
            "release_tmp_root=\"$RUNNER_TEMP/m80-release-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-publish\"",
            "upload_dir=\"$release_tmp_root/upload\"",
            "prepublish_dir=\"$release_tmp_root/prepublish\"",
            "redownload_dir=\"$release_tmp_root/redownload\"",
            "test ! -e \"$release_tmp_root\"",
            "mkdir -p \"$upload_dir\" \"$prepublish_dir\" \"$redownload_dir\"",
            "test ! -L \"$release_tmp_root\"",
            "owner_uid=\"$(stat -c '%u' \"$release_tmp_root\")\"",
            "current_uid=\"$(id -u)\"",
            "test \"$owner_uid\" = \"$current_uid\"",
            "mode_octal=\"$(stat -c '%a' \"$release_tmp_root\")\"",
            "test \"$mode_octal\" = \"700\"",
            "echo \"RELEASE_TMP_ROOT=$release_tmp_root\"",
            "echo \"UPLOAD_DIR=$upload_dir\"",
            "rm -rf \"$RELEASE_TMP_ROOT\"",
        ],
    }
    for job_id, required_tokens in required_by_job.items():
        block = job_block_text(lines, job_id)
        block_text = "" if block is None else "\n".join(block)
        for token in required_tokens:
            if token not in block_text:
                errors.append(f"{path}: release job {job_id} must isolate and validate runner temp scratch root")
                break
    return errors


def lint_timeout_budget(
    path: Path,
    *,
    line_no: int,
    subject: str,
    minutes: int,
) -> list[str]:
    if minutes < 1:
        return [f"{path}:{line_no}: {subject} timeout-minutes must be at least 1"]
    if minutes > MAX_RELEASE_JOB_TIMEOUT_MINUTES:
        return [
            f"{path}:{line_no}: {subject} timeout-minutes {minutes} exceeds maximum "
            f"{MAX_RELEASE_JOB_TIMEOUT_MINUTES}"
        ]
    return []


def lint_multiline_run_block_strictness(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for index, line in enumerate(lines):
        match = MULTILINE_RUN_RE.match(line)
        if not match:
            continue
        run_indent = len(match.group(1))
        block_start = index + 1
        block_end = run_block_end(lines, block_start, run_indent)
        block = lines[block_start:block_end]
        if run_block_has_exception(block):
            continue
        first_command = first_run_block_command(block)
        if first_command is None:
            continue
        if first_command != RUN_BLOCK_STRICT_PREAMBLE:
            errors.append(
                f'{path}:{index + 1}: multiline run block must start with "{RUN_BLOCK_STRICT_PREAMBLE}" '
                f"or include {RUN_BLOCK_EXCEPTION_MARKER}"
            )
            continue
        errors.extend(lint_masked_command_substitutions(path, block_start, block))
    return errors


def run_block_end(lines: list[str], block_start: int, run_indent: int) -> int:
    for index in range(block_start, len(lines)):
        line = lines[index]
        if not line.strip():
            continue
        if leading_spaces(line) <= run_indent:
            return index
    return len(lines)


def run_block_has_exception(block: list[str]) -> bool:
    for line in block:
        stripped = line.strip()
        if not stripped:
            continue
        if stripped.startswith("#"):
            return RUN_BLOCK_EXCEPTION_MARKER in stripped
        return False
    return False


def first_run_block_command(block: list[str]) -> str | None:
    for line in block:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        return stripped
    return None


def lint_masked_command_substitutions(
    path: Path,
    block_start: int,
    block: list[str],
) -> list[str]:
    errors: list[str] = []
    for offset, line in enumerate(block):
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or COMMAND_SUBSTITUTION_RE.search(stripped) is None:
            continue
        if ASSIGNMENT_COMMAND_SUBSTITUTION_RE.match(stripped):
            continue
        errors.append(
            f"{path}:{block_start + offset + 1}: command substitution in workflow run block "
            "must be captured in a standalone assignment before use"
        )
    return errors


def leading_spaces(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def rustup_target_toolchain(line: str) -> str | None:
    parts = line.split()
    for index, part in enumerate(parts):
        if part == "--toolchain" and index + 1 < len(parts):
            return parts[index + 1]
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


def job_environment_name(lines: list[str]) -> str | None:
    for index, line in enumerate(lines):
        if not line.startswith("    environment:"):
            continue
        suffix = line.split(":", 1)[1].strip()
        if suffix:
            return strip_quotes(suffix)
        for nested in lines[index + 1 :]:
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if not nested.startswith("      "):
                break
            stripped = nested.strip()
            if stripped.startswith("name:"):
                return strip_quotes(stripped.split(":", 1)[1].strip())
        return None
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


def top_level_concurrency_cancel_in_progress(lines: list[str]) -> str | None:
    for index, line in enumerate(lines):
        if line != "concurrency:":
            continue
        for nested in lines[index + 1 :]:
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if not nested.startswith("  "):
                break
            stripped = nested.strip()
            if stripped.startswith("cancel-in-progress:"):
                return stripped.split(":", 1)[1].strip()
        return None
    return None


def has_schedule_event(lines: list[str]) -> bool:
    return has_event(lines, "schedule")


def has_workflow_dispatch_event(lines: list[str]) -> bool:
    return has_event(lines, "workflow_dispatch")


def has_event(lines: list[str], event: str) -> bool:
    for index, line in enumerate(lines):
        if line.startswith("on:") and event in line:
            return True
        if line.startswith("on:"):
            for nested in lines[index + 1 :]:
                if not nested.strip() or nested.lstrip().startswith("#"):
                    continue
                if not nested.startswith("  "):
                    break
                if nested.strip().startswith(f"{event}:"):
                    return True
    return False


def workflow_dispatch_declares_inputs(lines: list[str]) -> bool:
    for index, line in enumerate(lines):
        if line.startswith("on:") and "workflow_dispatch" in line:
            continue
        if not line.strip().startswith("workflow_dispatch:"):
            continue
        for nested in lines[index + 1 :]:
            if not nested.strip() or nested.lstrip().startswith("#"):
                continue
            if leading_spaces(nested) <= leading_spaces(line):
                break
            if nested.strip().startswith("inputs:"):
                return True
    return False


def job_block_text(lines: list[str], job_id: str) -> list[str] | None:
    for current_job_id, start, end in job_blocks(lines):
        if current_job_id == job_id:
            return lines[start:end]
    return None


def has_always_artifact_upload(lines: list[str]) -> bool:
    for index, line in enumerate(lines):
        if "uses: actions/upload-artifact@" not in line:
            continue
        step_start = previous_step_index(lines, index)
        if step_start is None:
            continue
        block_end = next_step_index(lines, index + 1)
        step = "\n".join(lines[step_start:block_end])
        required_artifacts = [
            "m80-latest-freshness-proof.json",
            "m80-latest-freshness-drift.json",
            "m80-latest-freshness.stdout",
            "m80-latest-freshness.stderr",
        ]
        if "if: always()" in step and all(artifact in step for artifact in required_artifacts):
            return True
    return False


def has_freshness_release_publish(lines: list[str]) -> bool:
    for job_id, start, end in job_blocks(lines):
        if not is_publish_job(job_id):
            continue
        block = "\n".join(lines[start:end])
        required = [
            "needs: hostless-public-freshness",
            "needs.hostless-public-freshness.result == 'success'",
            "permissions:",
            "contents: write",
            "actions/download-artifact@",
            "m80-latest-freshness-${{ github.run_id }}",
            "m80-latest-freshness-proof.json",
            "resolved_tag",
            "gh release upload",
            "--clobber",
            "--repo \"$GITHUB_REPOSITORY\"",
        ]
        if all(token in block for token in required):
            return True
    return False


def previous_step_index(lines: list[str], index: int) -> int | None:
    for current in range(index, -1, -1):
        if re.match(r"^\s{6}-\s", lines[current]):
            return current
    return None


def next_step_index(lines: list[str], index: int) -> int:
    for current in range(index, len(lines)):
        if re.match(r"^\s{6}-\s", lines[current]):
            return current
    return len(lines)


def is_trusted_first_party_action(repo: str) -> bool:
    return repo.startswith("actions/")


def workflow_needs_release_guards(path: Path) -> bool:
    return any(token in path.name for token in GUARDED_FILENAME_TOKENS)


def job_uses_reusable_workflow(lines: list[str]) -> bool:
    return any(line.startswith("    uses:") for line in lines)


def reusable_timeout_marker_minutes(
    path: Path,
    job_id: str,
    job_start: int,
    lines: list[str],
) -> tuple[int | None, list[str], int]:
    errors: list[str] = []
    found_line = job_start + 1
    for offset, line in enumerate(lines):
        stripped = line.strip()
        if "m80-lint: reusable-timeout-minutes" not in stripped:
            continue
        found_line = job_start + offset + 1
        if not stripped.startswith("#"):
            errors.append(
                f"{path}:{found_line}: release reusable job {job_id} timeout marker "
                "must be a YAML comment"
            )
            continue
        match = REUSABLE_TIMEOUT_MARKER_RE.search(stripped)
        if match is None:
            errors.append(
                f"{path}:{found_line}: release reusable job {job_id} timeout marker "
                "must use m80-lint: reusable-timeout-minutes=N"
            )
            continue
        return int(match.group(1)), errors, found_line
    return None, errors, found_line


def is_publish_job(job_id: str) -> bool:
    return job_id == "publish" or job_id.startswith("publish-")


def is_attestation_build_job(job_id: str) -> bool:
    return job_id == "build-release-artifacts"


def is_attestation_build_permission(job_id: str, scope: str) -> bool:
    return is_attestation_build_job(job_id) and scope in {"id-token", "attestations"}


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
