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
CARGO_COMMAND_RE = re.compile(r"(?:^|\s)cargo(?:\s+\+\S+)?\s+(build|test|clippy|install)\b")
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
REUSABLE_TIMEOUT_MARKER_RE = re.compile(
    r"m80-lint:\s*reusable-timeout-minutes=([0-9]+)\b"
)


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
        errors.extend(lint_attestation_permissions(path, lines, release_workflow=release_workflow))
        errors.extend(lint_multiline_run_block_strictness(path, lines))
        if is_freshness_workflow(path):
            errors.extend(lint_freshness_workflow(path, text, lines))
        if release_workflow:
            errors.extend(lint_release_concurrency(path, lines))
            errors.extend(lint_release_job_timeouts(path, lines))
            errors.extend(lint_release_cargo_locked(path, lines))
            errors.extend(lint_release_rust_toolchain_pins(path, lines))
            errors.extend(lint_release_artifact_origin(path, text, lines))
            errors.extend(lint_release_temp_isolation(path, text, lines))
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


def lint_release_job_timeouts(path: Path, lines: list[str]) -> list[str]:
    errors: list[str] = []
    for job_id, start, end in job_blocks(lines):
        block = lines[start:end]
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
            continue

        errors.append(f"{path}:{start + 1}: release job {job_id} must declare timeout-minutes")
    return errors


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
        if run_block_has_exception(lines, index, block):
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


def run_block_has_exception(lines: list[str], run_index: int, block: list[str]) -> bool:
    del lines, run_index
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
    return any(
        token in path.name
        for token in ["release", "latest", "freshness", "proof", "publish"]
    )


def is_freshness_workflow(path: Path) -> bool:
    return "freshness" in path.name


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
