#!/usr/bin/env python3
"""Verify release/quickstart tracker proof-close policy."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any


DEFAULT_EPIC = "m80-o3uh9"
VERIFIED_LABEL = "requires-verified-close"
POLICY_CONFIG_SCHEMA_VERSION = 1
POLICY_CONFIG_TOP_LEVEL_KEYS = {"schema_version", "epochs"}
POLICY_CONFIG_EPOCH_KEYS = {"id", "status", "reason"}
POLICY_CONFIG_STATUSES = {"active", "retired"}
CLOSE_MATRIX_KIND = "release_final_close_matrix"
CLOSE_MATRIX_SCHEMA_VERSION = 1
CLOSE_MATRIX_TOP_LEVEL_KEYS = {
    "schema_version",
    "kind",
    "epoch_id",
    "tracker_digest",
    "generated_at",
    "rows",
}
CLOSE_MATRIX_ROW_KEYS = {
    "id",
    "status",
    "behavior_doc",
    "test_command",
    "proof_artifact",
    "exception_reason",
    "requires_verified_close",
    "requires_real_substrate",
}
CLOSE_MATRIX_STATUSES = {"open", "in_progress", "blocked", "closed", "deferred", "tombstone"}
POLICY_EFFECTIVE_AT = datetime(2026, 5, 20, 18, 0, 0, tzinfo=timezone.utc)
VERIFIED_REF_RE = re.compile(r"verified:\s+([^\s]+)\s+@\s+([0-9a-fA-F]{7,40})\b")
SHA256_DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
TRACKED_TEXT_FIELDS = ("title", "description", "acceptance_criteria", "close_reason")
EXPECTED_INSTALL_REPLACEMENT = (
    "use latest/pinned release install.sh or m80 install; reserve "
    "m80 quickstart --artifact-url for local/operator/test overrides"
)
RAW_MAIN_INSTALL_URL_RE = re.compile(
    r"https://(?:raw\.githubusercontent\.com/[^/\s`\"')]+/m80/(?:main|master)/[^\s`\"')]+|"
    r"github\.com/[^/\s`\"')]+/m80/(?:raw|blob)/(?:main|master)/[^\s`\"')]+)",
    re.IGNORECASE,
)
GITHUB_RELEASE_DOWNLOAD_RE = re.compile(
    r"https://github\.com/([^/\s`\"')]+)/([^/\s`\"')]+)/releases/"
    r"(?:download/[^/\s`\"')]+|latest/download)/([^\s`\"')]+)",
    re.IGNORECASE,
)
QUICKSTART_RE = re.compile(r"\bm80\s+quickstart\b", re.IGNORECASE)
QUICKSTART_OVERRIDE_MARKER_RE = re.compile(
    r"\b(local|fixture|operator|test|override|legacy|internal|explicit)\b",
    re.IGNORECASE,
)
QUICKSTART_ALLOWED_FLAG_RE = re.compile(
    r"\b(?:sudo\s+)?m80\s+quickstart\b[^\n]*(--artifact-url|--no-run)\b",
    re.IGNORECASE,
)
QUICKSTART_POLICY_META_RE = re.compile(
    r"\b(fail|fails|reject|rejects|forbid|forbids|scan|scans|lint|inventory|"
    r"reintroduce|deprecated|stale|no-arg|template)\b",
    re.IGNORECASE,
)
PUBLIC_SELECTOR_RE = re.compile(
    r"\b(public|common|first-run|normal|user-facing|latest|pinned|select|selector|install path)\b",
    re.IGNORECASE,
)
PROOF_EXEMPT_TITLE_RE = re.compile(r"\b(policy lint|anti-drift lint)\b", re.IGNORECASE)
REAL_KVM_RE = re.compile(
    r"real[- ]?kvm[^.\n]*(proof|smoke|baseline|runner|quickstart|release|process-wrapper)|"
    r"(proof|smoke|baseline|runner|quickstart|release|process-wrapper)[^.\n]*real[- ]?kvm",
    re.IGNORECASE,
)
NEGATED_REAL_KVM_RE = re.compile(
    r"(without requiring|does not require|no)[^.\n]*real[- ]?kvm",
    re.IGNORECASE,
)
PROOF_REQUIRING_RE = re.compile(
    r"real-substrate|real substrate|external substrate|"
    r"public latest|public release access|documented curl works|"
    r"hostless release fixture|hostless proof|hostless[^.\n]*quickstart proof|"
    r"quickstart proof|proof ledger|release proof|release-proof|"
    r"close matrix[^.\n]*proof",
    re.IGNORECASE,
)
PUBLIC_PROOF_REQUIRING_RE = re.compile(
    r"public[- ]latest[^.\n]*(proof|receipt)|"
    r"(proof|receipt)[^.\n]*public[- ]latest|"
    r"public[- ]release[^.\n]*proof|"
    r"proof[^.\n]*public[- ]release|"
    r"public[- ]access[^.\n]*(proof|receipt|lane)|"
    r"(proof|receipt|lane)[^.\n]*public[- ]access|"
    r"fixture (receipts|substrate)[^.\n]*(cannot|must not) satisfy|"
    r"real public[- ]access lane|"
    r"verified close[^.\n]*real public release",
    re.IGNORECASE,
)
PUBLIC_PROOF_EXCLUSION_RE = re.compile(
    r"(excluded from|not uploaded as|must not be|are not|is not)[^.\n]*"
    r"(public[- ]release|public release|public asset|public download)",
    re.IGNORECASE,
)
FRESHNESS_PUBLIC_PROOF_RE = re.compile(
    r"public asset|installer-consumed public asset|published sums",
    re.IGNORECASE,
)
PUBLIC_PROOF_EXEMPT_TITLE_RE = re.compile(
    r"\b(guard|policy|lint|schema|scaffold|fixture-only)\b",
    re.IGNORECASE,
)
PUBLIC_SUBSTRATE_RE = re.compile(
    r"public[- ]github|public github|public[- ]unauthenticated|"
    r"unauthenticated public|public[- ]latest|public release|"
    r"unauthenticated bounded curl",
    re.IGNORECASE,
)
FIXTURE_SUBSTRATE_RE = re.compile(
    r"\b(fixture|fake|scaffold|hostless-fixture)\b",
    re.IGNORECASE,
)
LOCAL_SUBSTRATE_RE = re.compile(r"\blocal\b", re.IGNORECASE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--issues",
        type=Path,
        default=Path(".beads/issues.jsonl"),
        help="beads JSONL export to lint",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root for artifact and git checks",
    )
    parser.add_argument(
        "--epic",
        default=None,
        help=f"release epoch id to lint without --policy-config (default: {DEFAULT_EPIC})",
    )
    parser.add_argument(
        "--policy-config",
        type=Path,
        default=None,
        help="machine-readable release tracker policy config to lint",
    )
    parser.add_argument(
        "--skip-git-history",
        action="store_true",
        help="skip git commit/object checks for fixture-only invocations",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    issues = read_issues(args.issues)
    if args.policy_config is not None:
        if args.epic is not None:
            print("use --epic or --policy-config, not both", file=sys.stderr)
            return 2
        errors = verify_policy_config(
            issues,
            config=read_policy_config(args.policy_config),
            config_path=args.policy_config,
            repo_root=repo_root,
            check_git_history=not args.skip_git_history,
        )
    else:
        errors = verify_tracker_policy(
            issues,
            repo_root=repo_root,
            epic=args.epic or DEFAULT_EPIC,
            check_git_history=not args.skip_git_history,
        )
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    if args.policy_config is not None:
        print(f"release tracker policy ok: {args.policy_config}")
    else:
        print(f"release tracker policy ok: {args.issues}")
    return 0


def read_issues(path: Path) -> list[dict[str, Any]]:
    issues: list[dict[str, Any]] = []
    with path.open() as f:
        for line_no, line in enumerate(f, start=1):
            if not line.strip():
                continue
            try:
                issue = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"{path}:{line_no}: invalid JSON: {exc}") from exc
            if not isinstance(issue, dict):
                raise SystemExit(f"{path}:{line_no}: issue row must be a JSON object")
            issues.append(issue)
    return issues


def read_policy_config(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: policy config must be a JSON object")
    return value


def verify_policy_config(
    issues: list[dict[str, Any]],
    *,
    config: dict[str, Any],
    config_path: Path,
    repo_root: Path,
    check_git_history: bool,
) -> list[str]:
    errors = validate_policy_config(config, config_path)
    if errors:
        return errors

    issues_by_id = {
        issue["id"]: issue
        for issue in issues
        if isinstance(issue.get("id"), str)
    }
    errors.extend(verify_config_covers_active_epochs(issues_by_id, config, config_path))
    active_epochs = [
        epoch["id"]
        for epoch in config["epochs"]
        if epoch["status"] == "active"
    ]
    if not active_epochs:
        return [f"{config_path}: must name at least one active epoch"]

    for epic in active_epochs:
        if epic not in issues_by_id:
            errors.append(f"{config_path}: active epoch {epic} is missing from tracker")
            continue
        for error in verify_tracker_policy(
            issues,
            repo_root=repo_root,
            epic=epic,
            check_git_history=check_git_history,
        ):
            errors.append(f"{config_path}: active epoch {epic}: {error}")
    for error in verify_closed_configured_epochs(
        issues_by_id,
        config=config,
        config_path=config_path,
        repo_root=repo_root,
        check_git_history=check_git_history,
    ):
        errors.append(error)
    return errors


def verify_closed_configured_epochs(
    issues_by_id: dict[str, dict[str, Any]],
    *,
    config: dict[str, Any],
    config_path: Path,
    repo_root: Path,
    check_git_history: bool,
) -> list[str]:
    errors: list[str] = []
    parent_by_child = parent_links(issues_by_id)
    for epoch in config["epochs"]:
        epoch_id = epoch["id"]
        issue = issues_by_id.get(epoch_id)
        if issue is None or not is_closed_release_epoch(issue):
            continue
        for error in verify_closed_epoch_close_matrix(
            issue,
            issues_by_id=issues_by_id,
            parent_by_child=parent_by_child,
            repo_root=repo_root,
            check_git_history=check_git_history,
        ):
            errors.append(f"{config_path}: configured epoch {epoch_id}: {error}")
    return errors


def verify_config_covers_active_epochs(
    issues_by_id: dict[str, dict[str, Any]],
    config: dict[str, Any],
    config_path: Path,
) -> list[str]:
    configured = {
        epoch["id"]: epoch["status"]
        for epoch in config["epochs"]
    }
    errors: list[str] = []
    for issue_id, issue in sorted(issues_by_id.items()):
        if not is_active_release_epoch(issue):
            continue
        status = configured.get(issue_id)
        if status is None:
            errors.append(
                f"{config_path}: active tracker epoch {issue_id} is missing from policy config"
            )
        elif status == "retired":
            errors.append(
                f"{config_path}: active tracker epoch {issue_id} is configured as retired"
            )
    return errors


def is_active_release_epoch(issue: dict[str, Any]) -> bool:
    labels = issue.get("labels")
    if not isinstance(labels, list):
        return False
    label_set = set(labels)
    return (
        is_openish(issue)
        and "epoch" in label_set
        and ("release" in label_set or "quickstart" in label_set)
    )


def is_closed_release_epoch(issue: dict[str, Any]) -> bool:
    labels = issue.get("labels")
    if not isinstance(labels, list):
        return False
    label_set = set(labels)
    return (
        issue.get("status") == "closed"
        and "epoch" in label_set
        and ("release" in label_set or "quickstart" in label_set)
    )


def validate_policy_config(config: dict[str, Any], config_path: Path) -> list[str]:
    errors: list[str] = []
    unknown_keys = sorted(set(config) - POLICY_CONFIG_TOP_LEVEL_KEYS)
    if unknown_keys:
        errors.append(f"{config_path}: unknown top-level keys: {', '.join(unknown_keys)}")
    if config.get("schema_version") != POLICY_CONFIG_SCHEMA_VERSION:
        errors.append(
            f"{config_path}: schema_version must be {POLICY_CONFIG_SCHEMA_VERSION}"
        )
    epochs = config.get("epochs")
    if not isinstance(epochs, list):
        errors.append(f"{config_path}: epochs must be a list")
        return errors
    if not epochs:
        errors.append(f"{config_path}: epochs must not be empty")
        return errors

    seen: set[str] = set()
    for index, epoch in enumerate(epochs):
        prefix = f"{config_path}: epochs[{index}]"
        if not isinstance(epoch, dict):
            errors.append(f"{prefix}: entry must be a JSON object")
            continue
        unknown_epoch_keys = sorted(set(epoch) - POLICY_CONFIG_EPOCH_KEYS)
        if unknown_epoch_keys:
            errors.append(f"{prefix}: unknown keys: {', '.join(unknown_epoch_keys)}")
        epoch_id = epoch.get("id")
        if not nonempty_str(epoch_id):
            errors.append(f"{prefix}: id must be a nonempty string")
        elif epoch_id in seen:
            errors.append(f"{prefix}: duplicate epoch id {epoch_id}")
        else:
            seen.add(epoch_id)

        status = epoch.get("status")
        if status not in POLICY_CONFIG_STATUSES:
            errors.append(f"{prefix}: status must be active or retired")
        if status == "active" and "reason" in epoch:
            errors.append(f"{prefix}: active epochs must not carry a retired reason")
        if status == "retired" and not nonempty_str(epoch.get("reason")):
            errors.append(f"{prefix}: retired epochs require a nonempty reason")
    return errors


def verify_tracker_policy(
    issues: list[dict[str, Any]],
    *,
    repo_root: Path,
    epic: str,
    check_git_history: bool,
) -> list[str]:
    issues_by_id = {
        issue["id"]: issue
        for issue in issues
        if isinstance(issue.get("id"), str)
    }
    release_issues = {
        issue_id: issue
        for issue_id, issue in issues_by_id.items()
        if issue_id == epic or issue_id.startswith(f"{epic}.")
    }
    parent_by_child = parent_links(release_issues)
    required_ids = required_verified_close_ids(release_issues, parent_by_child, epic)

    errors: list[str] = []
    for issue_id in sorted(required_ids):
        issue = release_issues[issue_id]
        if not carries_verified_label(issue):
            errors.append(f"{issue_id}: must carry {VERIFIED_LABEL}")

    for issue_id, issue in sorted(release_issues.items()):
        errors.extend(verify_tracker_text_contract(issue))
        if issue.get("status") != "closed" or not carries_verified_label(issue):
            continue
        errors.extend(
            verify_closed_issue(
                issue,
                repo_root=repo_root,
                check_git_history=check_git_history,
            )
        )
    return errors


def verify_tracker_text_contract(issue: dict[str, Any]) -> list[str]:
    issue_id = str(issue.get("id") or "<unknown>")
    errors: list[str] = []
    for field in TRACKED_TEXT_FIELDS:
        value = issue.get(field)
        if not isinstance(value, str) or not value:
            continue
        errors.extend(verify_text_field_contract(issue_id, field, value))
    return errors


def verify_text_field_contract(issue_id: str, field: str, text: str) -> list[str]:
    errors: list[str] = []
    for match in RAW_MAIN_INSTALL_URL_RE.finditer(text):
        errors.append(
            tracker_contract_error(
                issue_id,
                field,
                match.group(0),
                "raw main installer URLs are mutable",
            )
        )

    for match in GITHUB_RELEASE_DOWNLOAD_RE.finditer(text):
        owner, repo, asset = match.groups()
        url = match.group(0)
        if looks_like_m80_release_asset(asset) and (owner, repo) != ("moradology", "m80"):
            errors.append(
                tracker_contract_error(
                    issue_id,
                    field,
                    url,
                    "public m80 release downloads must use moradology/m80",
                )
            )
        if is_artifact_only_latest_url(url, asset):
            errors.append(
                tracker_contract_error(
                    issue_id,
                    field,
                    url,
                    "artifact-only releases/latest URLs are not the public install selector",
                )
            )

    for line in text.splitlines():
        if QUICKSTART_RE.search(line) is None:
            continue
        if quickstart_line_is_allowed(line):
            continue
        errors.append(
            tracker_contract_error(
                issue_id,
                field,
                line.strip() or "m80 quickstart",
                "no-arg m80 quickstart is not the public release selector",
            )
        )
    return errors


def looks_like_m80_release_asset(asset: str) -> bool:
    return asset == "install.sh" or asset.startswith("m80-") or "m80" in asset


def is_artifact_only_latest_url(url: str, asset: str) -> bool:
    if "/releases/latest/download/" not in url.lower():
        return False
    if asset == "install.sh":
        return False
    return asset.endswith(".tar.gz") or "artifact" in asset or "bundle" in asset


def quickstart_line_is_allowed(line: str) -> bool:
    if QUICKSTART_POLICY_META_RE.search(line) is not None:
        return True
    if QUICKSTART_ALLOWED_FLAG_RE.search(line) is not None:
        return True
    if QUICKSTART_OVERRIDE_MARKER_RE.search(line) is not None:
        return True
    return PUBLIC_SELECTOR_RE.search(line) is None


def tracker_contract_error(issue_id: str, field: str, offending: str, reason: str) -> str:
    return (
        f"{issue_id}: {field} contains unsupported release command claim "
        f"{offending!r}: {reason}; expected {EXPECTED_INSTALL_REPLACEMENT}"
    )


def parent_links(issues_by_id: dict[str, dict[str, Any]]) -> dict[str, str]:
    parents: dict[str, str] = {}
    for issue_id, issue in issues_by_id.items():
        for dependency in issue.get("dependencies") or []:
            if not isinstance(dependency, dict):
                continue
            if dependency.get("type") != "parent-child":
                continue
            parent_id = dependency.get("depends_on_id")
            if isinstance(parent_id, str):
                parents[issue_id] = parent_id
    return parents


def required_verified_close_ids(
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
    epic: str,
) -> set[str]:
    required: set[str] = set()
    for issue_id, issue in issues_by_id.items():
        if not is_subject_to_label_policy(issue):
            continue
        if issue_id == epic:
            continue
        if issue_requires_verified_close(issue):
            required.add(issue_id)
            required.update(open_ancestors(issue_id, issues_by_id, parent_by_child, epic))
    return required


def open_ancestors(
    issue_id: str,
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
    epic: str,
) -> set[str]:
    ancestors: set[str] = set()
    current = issue_id
    while current in parent_by_child:
        parent_id = parent_by_child[current]
        parent = issues_by_id.get(parent_id)
        if parent is None:
            break
        if is_openish(parent):
            ancestors.add(parent_id)
        if parent_id == epic:
            break
        current = parent_id
    return ancestors


def issue_requires_verified_close(issue: dict[str, Any]) -> bool:
    if carries_verified_label(issue):
        return True
    title = str(issue.get("title") or "")
    if PROOF_EXEMPT_TITLE_RE.search(title):
        return False
    labels = set(issue.get("labels") or [])
    if "real-kvm" in labels:
        return True
    text = f"{title}\n{issue.get('description') or ''}"
    if REAL_KVM_RE.search(text) is not None and NEGATED_REAL_KVM_RE.search(text) is None:
        return True
    return PROOF_REQUIRING_RE.search(text) is not None


def carries_verified_label(issue: dict[str, Any]) -> bool:
    labels = issue.get("labels") or []
    return isinstance(labels, list) and VERIFIED_LABEL in labels


def is_openish(issue: dict[str, Any]) -> bool:
    return issue.get("status") not in {"closed", "tombstone", "deferred"}


def is_subject_to_label_policy(issue: dict[str, Any]) -> bool:
    if is_openish(issue):
        return True
    if issue.get("status") != "closed":
        return False
    timestamp = issue_policy_timestamp(issue)
    return timestamp is not None and timestamp >= POLICY_EFFECTIVE_AT


def issue_policy_timestamp(issue: dict[str, Any]) -> datetime | None:
    timestamps = [
        parse_timestamp(issue.get(field))
        for field in ["closed_at", "updated_at", "created_at"]
    ]
    timestamps = [value for value in timestamps if value is not None]
    if not timestamps:
        return None
    return max(timestamps)


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


def verify_closed_issue(
    issue: dict[str, Any],
    *,
    repo_root: Path,
    check_git_history: bool,
) -> list[str]:
    issue_id = issue["id"]
    reason = issue.get("close_reason")
    if not isinstance(reason, str) or not reason.strip():
        return [f"{issue_id}: closed {VERIFIED_LABEL} issue missing close_reason"]
    refs = VERIFIED_REF_RE.findall(reason)
    if not refs:
        return [
            f"{issue_id}: close_reason must contain "
            "`verified: <artifact-path> @ <commit-sha>`"
        ]

    errors: list[str] = []
    for rel_path, commit in refs:
        artifact = artifact_path(repo_root, rel_path)
        if artifact is None:
            errors.append(f"{issue_id}: verified artifact path must be relative: {rel_path}")
            continue
        if not artifact.is_file():
            errors.append(f"{issue_id}: verified artifact path is missing: {rel_path}")
            continue
        if check_git_history:
            errors.extend(verify_git_ref(repo_root, issue_id, rel_path, commit))
        errors.extend(verify_proof_artifact(issue, artifact))
    return errors


def verify_closed_epoch_close_matrix(
    issue: dict[str, Any],
    *,
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
    repo_root: Path,
    check_git_history: bool,
) -> list[str]:
    issue_id = issue["id"]
    reason = issue.get("close_reason")
    if not isinstance(reason, str) or not reason.strip():
        return [f"{issue_id}: closed release epoch requires final close-matrix close_reason"]
    refs = VERIFIED_REF_RE.findall(reason)
    if not refs:
        return [
            f"{issue_id}: close_reason must contain "
            "`verified: <artifact-path> @ <commit-sha>` for a final close matrix"
        ]

    errors: list[str] = []
    for rel_path, commit in refs:
        errors.extend(verify_close_matrix_ref(
            issue_id,
            rel_path,
            commit,
            epoch_id=issue_id,
            issues_by_id=issues_by_id,
            parent_by_child=parent_by_child,
            repo_root=repo_root,
            check_git_history=check_git_history,
        ))
    return errors


def verify_close_matrix_ref(
    issue_id: str,
    rel_path: str,
    commit: str,
    *,
    epoch_id: str,
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
    repo_root: Path,
    check_git_history: bool,
) -> list[str]:
    artifact = artifact_path(repo_root, rel_path)
    if artifact is None:
        return [f"{issue_id}: final close matrix path must be relative: {rel_path}"]
    if not artifact.is_file():
        return [f"{issue_id}: final close matrix path is missing: {rel_path}"]
    errors: list[str] = []
    if check_git_history:
        errors.extend(verify_git_ref(repo_root, issue_id, rel_path, commit))
        if errors:
            return errors
    try:
        value = json.loads(artifact.read_text(errors="replace"))
    except json.JSONDecodeError as exc:
        return [f"{issue_id}: final close matrix must be JSON: {rel_path}: {exc}"]
    if not isinstance(value, dict) or value.get("kind") != CLOSE_MATRIX_KIND:
        return [
            f"{issue_id}: final close matrix artifact must have kind "
            f"{CLOSE_MATRIX_KIND}: {rel_path}"
        ]
    errors = verify_close_matrix_artifact(issue_id, artifact, value)
    if errors:
        return errors
    return verify_close_matrix_completeness(
        issue_id,
        artifact,
        value,
        epoch_id=epoch_id,
        issues_by_id=issues_by_id,
        parent_by_child=parent_by_child,
    )


def artifact_path(repo_root: Path, rel_path: str) -> Path | None:
    if not safe_relative_path(rel_path):
        return None
    return repo_root / rel_path


def safe_relative_path(value: str) -> bool:
    path = Path(value)
    return bool(value) and not path.is_absolute() and ".." not in path.parts


def verify_git_ref(repo_root: Path, issue_id: str, rel_path: str, commit: str) -> list[str]:
    errors: list[str] = []
    if git(repo_root, "cat-file", "-e", f"{commit}^{{commit}}").returncode != 0:
        return [f"{issue_id}: verified commit does not exist: {commit}"]
    if git(repo_root, "cat-file", "-e", f"{commit}:{rel_path}").returncode != 0:
        errors.append(f"{issue_id}: verified commit {commit} does not contain {rel_path}")
    return errors


def git(repo_root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repo_root), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def verify_proof_artifact(issue: dict[str, Any], path: Path) -> list[str]:
    issue_id = string_field(issue, "id") or "<unknown>"
    text = path.read_text(errors="replace")
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        errors = verify_text_proof_artifact(issue_id, path, text)
        if not errors:
            errors.extend(verify_public_text_proof_substrate(issue, path, text))
        return errors
    if not isinstance(value, dict):
        return [f"{issue_id}: verified artifact must be a JSON object or proof text: {path}"]
    if value.get("kind") == CLOSE_MATRIX_KIND:
        return verify_close_matrix_artifact(issue_id, path, value)

    errors: list[str] = []
    if quickstart_proof_artifact_is_complete(value):
        pass
    elif generic_proof_artifact_is_complete(value):
        pass
    else:
        errors.append(
            f"{issue_id}: verified artifact must contain command, stdout/stderr or log path, "
            f"exit status, resolved tag, and substrate: {path}"
        )
    if not errors:
        errors.extend(verify_public_json_proof_substrate(issue, path, value))
    return errors


def verify_public_json_proof_substrate(
    issue: dict[str, Any],
    path: Path,
    value: dict[str, Any],
) -> list[str]:
    if not issue_requires_public_substrate(issue):
        return []
    issue_id = string_field(issue, "id") or "<unknown>"
    if artifact_has_fixture_or_local_substrate(value):
        return [
            f"{issue_id}: verified artifact uses fixture/local substrate but issue "
            f"requires public latest/release proof: {path}"
        ]
    if not artifact_has_public_or_real_kvm_substrate(value):
        return [
            f"{issue_id}: verified artifact lacks public latest/release or real-KVM "
            f"substrate required by issue text: {path}"
        ]
    return []


def verify_public_text_proof_substrate(
    issue: dict[str, Any],
    path: Path,
    text: str,
) -> list[str]:
    if not issue_requires_public_substrate(issue):
        return []
    issue_id = string_field(issue, "id") or "<unknown>"
    if text_has_fixture_or_local_substrate(text):
        return [
            f"{issue_id}: verified text artifact uses fixture/local substrate but issue "
            f"requires public latest/release proof: {path}"
        ]
    if PUBLIC_SUBSTRATE_RE.search(text) is None and REAL_KVM_RE.search(text) is None:
        return [
            f"{issue_id}: verified text artifact lacks public latest/release or real-KVM "
            f"substrate required by issue text: {path}"
        ]
    return []


def issue_requires_public_substrate(issue: dict[str, Any]) -> bool:
    title = string_field(issue, "title")
    if PUBLIC_PROOF_EXEMPT_TITLE_RE.search(title):
        return False
    text = "\n".join(
        string_field(issue, field)
        for field in ["title", "description", "acceptance_criteria", "notes"]
    )
    text = PUBLIC_PROOF_EXCLUSION_RE.sub("", text)
    if PUBLIC_PROOF_REQUIRING_RE.search(text) is not None:
        return True
    return (
        "freshness" in title.lower()
        and FRESHNESS_PUBLIC_PROOF_RE.search(text) is not None
    )


def artifact_has_public_or_real_kvm_substrate(value: dict[str, Any]) -> bool:
    if artifact_has_real_kvm_substrate(value):
        return True
    proof_substrate = value.get("proof_substrate")
    if isinstance(proof_substrate, str) and PUBLIC_SUBSTRATE_RE.search(proof_substrate):
        return True
    substrate = value.get("substrate")
    if isinstance(substrate, str):
        return PUBLIC_SUBSTRATE_RE.search(substrate) is not None
    if not isinstance(substrate, dict):
        return False
    if (
        substrate.get("network_target") == "public-github-release"
        and substrate.get("auth_state") == "unauthenticated-public-read"
        and substrate.get("public_owner") == "moradology"
        and substrate.get("public_repo") == "m80"
        and substrate.get("fixture_source") is False
        and substrate.get("github_write_apis_available") is False
    ):
        return all(
            substrate.get(field) in {None, "url"}
            for field in ["latest_source_mode", "guard_source_mode"]
        )
    if substrate.get("kind") in {"public-github", "public-unauthenticated"}:
        return substrate.get("fixture") is False or substrate.get("fixture_source") is False
    return any(
        isinstance(substrate.get(field), str)
        and PUBLIC_SUBSTRATE_RE.search(substrate[field]) is not None
        for field in ["kind", "summary", "network_target", "auth_state"]
    )


def artifact_has_real_kvm_substrate(value: dict[str, Any]) -> bool:
    substrate = value.get("substrate")
    if isinstance(substrate, str):
        return (
            REAL_KVM_RE.search(substrate) is not None
            and NEGATED_REAL_KVM_RE.search(substrate) is None
        )
    if not isinstance(substrate, dict):
        return False
    text = " ".join(
        str(substrate.get(field) or "")
        for field in ["kind", "summary", "proof_kind", "network_target"]
    )
    return REAL_KVM_RE.search(text) is not None and NEGATED_REAL_KVM_RE.search(text) is None


def artifact_has_fixture_or_local_substrate(value: dict[str, Any]) -> bool:
    proof_substrate = value.get("proof_substrate")
    if (
        isinstance(proof_substrate, str)
        and text_has_fixture_or_local_substrate(proof_substrate)
    ):
        return True
    substrate = value.get("substrate")
    if isinstance(substrate, str):
        return text_has_fixture_or_local_substrate(substrate)
    if not isinstance(substrate, dict):
        return False
    if substrate.get("fixture") is True:
        return True
    if substrate.get("fixture_source") is True:
        return True
    for field in ["latest_source_mode", "guard_source_mode"]:
        mode = substrate.get(field)
        if isinstance(mode, str) and mode and mode != "url":
            return True
    kind_text = " ".join(
        str(substrate.get(field) or "")
        for field in ["kind", "proof_kind", "summary"]
    )
    if FIXTURE_SUBSTRATE_RE.search(kind_text):
        return True
    return (
        LOCAL_SUBSTRATE_RE.search(kind_text) is not None
        and not artifact_has_public_or_real_kvm_substrate(value)
    )


def text_has_fixture_or_local_substrate(text: str) -> bool:
    if FIXTURE_SUBSTRATE_RE.search(text):
        return True
    return (
        LOCAL_SUBSTRATE_RE.search(text) is not None
        and PUBLIC_SUBSTRATE_RE.search(text) is None
    )


def verify_close_matrix_artifact(
    issue_id: str,
    path: Path,
    value: dict[str, Any],
) -> list[str]:
    prefix = f"{issue_id}: close matrix {path}"
    errors: list[str] = []

    unknown_keys = sorted(set(value) - CLOSE_MATRIX_TOP_LEVEL_KEYS)
    if unknown_keys:
        errors.append(f"{prefix}: unknown top-level keys: {', '.join(unknown_keys)}")

    if value.get("schema_version") != CLOSE_MATRIX_SCHEMA_VERSION:
        errors.append(f"{prefix}: schema_version must be {CLOSE_MATRIX_SCHEMA_VERSION}")
    if value.get("kind") != CLOSE_MATRIX_KIND:
        errors.append(f"{prefix}: kind must be {CLOSE_MATRIX_KIND}")
    if not nonempty_str(value.get("epoch_id")):
        errors.append(f"{prefix}: epoch_id must be a nonempty string")
    digest = value.get("tracker_digest")
    if not isinstance(digest, str) or SHA256_DIGEST_RE.fullmatch(digest) is None:
        errors.append(f"{prefix}: tracker_digest must be sha256:<64 lowercase hex>")
    generated_at = value.get("generated_at")
    if parse_timestamp(generated_at) is None:
        errors.append(f"{prefix}: generated_at must be an ISO-8601 timestamp")

    rows = value.get("rows")
    if not isinstance(rows, list):
        errors.append(f"{prefix}: rows must be a list")
        return errors
    if not rows:
        errors.append(f"{prefix}: rows must not be empty")
        return errors

    seen: set[str] = set()
    for index, row in enumerate(rows):
        row_prefix = f"{prefix}: rows[{index}]"
        if not isinstance(row, dict):
            errors.append(f"{row_prefix}: row must be a JSON object")
            continue
        errors.extend(verify_close_matrix_row(row_prefix, row))
        row_id = row.get("id")
        if isinstance(row_id, str) and row_id:
            if row_id in seen:
                errors.append(f"{row_prefix}: duplicate row id {row_id}")
            else:
                seen.add(row_id)
    return errors


def verify_close_matrix_row(prefix: str, row: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    unknown_keys = sorted(set(row) - CLOSE_MATRIX_ROW_KEYS)
    if unknown_keys:
        errors.append(f"{prefix}: unknown keys: {', '.join(unknown_keys)}")
    if not nonempty_str(row.get("id")):
        errors.append(f"{prefix}: id must be a nonempty string")
    if row.get("status") not in CLOSE_MATRIX_STATUSES:
        errors.append(
            f"{prefix}: status must be one of {', '.join(sorted(CLOSE_MATRIX_STATUSES))}"
        )
    behavior_doc = row.get("behavior_doc")
    if behavior_doc is not None and (
        not isinstance(behavior_doc, str) or not safe_relative_path(behavior_doc)
    ):
        errors.append(f"{prefix}: behavior_doc must be a relative path when present")

    test_command = row.get("test_command")
    proof_artifact = row.get("proof_artifact")
    exception_reason = row.get("exception_reason")
    has_behavior_doc = isinstance(behavior_doc, str) and safe_relative_path(behavior_doc)
    has_test_command = nonempty_str(test_command)
    has_proof_artifact = isinstance(proof_artifact, str) and safe_relative_path(proof_artifact)
    has_exception_reason = nonempty_str(exception_reason)
    if test_command is not None and not has_test_command:
        errors.append(f"{prefix}: test_command must be a nonempty string when present")
    if proof_artifact is not None and not has_proof_artifact:
        errors.append(f"{prefix}: proof_artifact must be a relative path when present")
    if exception_reason is not None and not has_exception_reason:
        errors.append(f"{prefix}: exception_reason must be a nonempty string when present")
    if not ((has_behavior_doc and has_test_command) or has_proof_artifact or has_exception_reason):
        errors.append(
            f"{prefix}: row must include behavior_doc and test_command, "
            "proof_artifact, or exception_reason"
        )

    for field in ["requires_verified_close", "requires_real_substrate"]:
        if not isinstance(row.get(field), bool):
            errors.append(f"{prefix}: {field} must be a boolean")
    return errors


def verify_close_matrix_completeness(
    issue_id: str,
    path: Path,
    matrix: dict[str, Any],
    *,
    epoch_id: str,
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
) -> list[str]:
    prefix = f"{issue_id}: close matrix {path}"
    rows = matrix.get("rows")
    if not isinstance(rows, list):
        return [f"{prefix}: rows must be a list"]
    descendant_ids = epoch_descendant_ids(issues_by_id, parent_by_child, epoch_id)
    row_by_id = {
        row["id"]: row
        for row in rows
        if isinstance(row, dict) and isinstance(row.get("id"), str) and row.get("id")
    }

    errors: list[str] = []
    for descendant_id in sorted(descendant_ids - set(row_by_id)):
        errors.append(f"{prefix}: matrix omits descendant {descendant_id}")
    for row_id in sorted(set(row_by_id) - descendant_ids):
        errors.append(f"{prefix}: matrix row references unknown descendant {row_id}")

    for row_id in sorted(descendant_ids & set(row_by_id)):
        row = row_by_id[row_id]
        issue = issues_by_id[row_id]
        status = issue.get("status")
        if row.get("status") != status:
            errors.append(
                f"{prefix}: {row_id} status is stale: matrix has "
                f"{row.get('status')}, tracker has {status}"
            )
        if is_openish(issue):
            errors.append(f"{prefix}: {row_id} descendant remains open with status {status}")
        if status in {"deferred", "tombstone"} and not nonempty_str(row.get("exception_reason")):
            errors.append(f"{prefix}: {row_id} {status} row requires exception_reason")
        if issue_requires_verified_close(issue) and row.get("requires_verified_close") is not True:
            errors.append(f"{prefix}: {row_id} requires_verified_close must be true")
        if issue_requires_real_substrate(issue) and row.get("requires_real_substrate") is not True:
            errors.append(f"{prefix}: {row_id} requires_real_substrate must be true")
        if status != "tombstone" and not row_has_audit_evidence(row):
            errors.append(
                f"{prefix}: {row_id} row must include behavior_doc and test_command "
                "or proof_artifact"
            )
    expected_digest = tracker_digest_for_epoch(issues_by_id, parent_by_child, epoch_id)
    if matrix.get("tracker_digest") != expected_digest:
        errors.append(
            f"{prefix}: tracker_digest is stale: matrix has "
            f"{matrix.get('tracker_digest')}, tracker has {expected_digest}"
        )
    return errors


def epoch_descendant_ids(
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
    epoch_id: str,
) -> set[str]:
    descendants: set[str] = set()
    for issue_id in issues_by_id:
        current = issue_id
        seen: set[str] = set()
        while current in parent_by_child and current not in seen:
            seen.add(current)
            parent_id = parent_by_child[current]
            if parent_id == epoch_id:
                descendants.add(issue_id)
                break
            current = parent_id
    return descendants


def issue_requires_real_substrate(issue: dict[str, Any]) -> bool:
    labels = set(issue.get("labels") or [])
    if "real-kvm" in labels:
        return True
    text = f"{issue.get('title') or ''}\n{issue.get('description') or ''}"
    return REAL_KVM_RE.search(text) is not None and NEGATED_REAL_KVM_RE.search(text) is None


def row_has_audit_evidence(row: dict[str, Any]) -> bool:
    behavior_doc = row.get("behavior_doc")
    test_command = row.get("test_command")
    proof_artifact = row.get("proof_artifact")
    return (
        (
            isinstance(behavior_doc, str)
            and safe_relative_path(behavior_doc)
            and nonempty_str(test_command)
        )
        or (
            isinstance(proof_artifact, str)
            and safe_relative_path(proof_artifact)
        )
    )


def tracker_digest_for_epoch(
    issues_by_id: dict[str, dict[str, Any]],
    parent_by_child: dict[str, str],
    epoch_id: str,
) -> str:
    issue_ids = {epoch_id}
    issue_ids.update(epoch_descendant_ids(issues_by_id, parent_by_child, epoch_id))
    rows = [
        tracker_digest_row(issue_id, issues_by_id[issue_id], parent_by_child, epoch_id)
        for issue_id in sorted(issue_ids)
        if issue_id in issues_by_id
    ]
    payload = json.dumps(rows, sort_keys=True, separators=(",", ":"))
    return f"sha256:{hashlib.sha256(payload.encode('utf-8')).hexdigest()}"


def tracker_digest_row(
    issue_id: str,
    issue: dict[str, Any],
    parent_by_child: dict[str, str],
    epoch_id: str,
) -> dict[str, Any]:
    return {
        "id": issue_id,
        "title": string_field(issue, "title"),
        "description": string_field(issue, "description"),
        "acceptance_criteria": string_field(issue, "acceptance_criteria"),
        "status": string_field(issue, "status"),
        "labels": sorted(label for label in issue.get("labels", []) if isinstance(label, str)),
        "parent": parent_by_child.get(issue_id),
        "close_reason": digest_close_reason(
            issue_id,
            epoch_id,
            string_field(issue, "close_reason"),
        ),
        "closed_at": string_field(issue, "closed_at"),
    }


def digest_close_reason(issue_id: str, epoch_id: str, close_reason: str) -> str:
    if issue_id != epoch_id:
        return close_reason
    return VERIFIED_REF_RE.sub(
        lambda match: f"verified: {match.group(1)} @ <commit-sha>",
        close_reason,
    )


def string_field(issue: dict[str, Any], field: str) -> str:
    value = issue.get(field)
    return value if isinstance(value, str) else ""


def quickstart_proof_artifact_is_complete(value: dict[str, Any]) -> bool:
    release = value.get("release")
    command = value.get("command")
    substrate = value.get("substrate")
    return (
        isinstance(release, dict)
        and nonempty_str(release.get("resolved_tag"))
        and isinstance(command, dict)
        and nonempty_str(command.get("display"))
        and isinstance(command.get("observed_exit_status"), int)
        and isinstance(substrate, dict)
        and nonempty_str(substrate.get("kind"))
        and ("stdout" in value)
        and ("stderr" in value or "log_path" in value or "logs" in value)
    )


def generic_proof_artifact_is_complete(value: dict[str, Any]) -> bool:
    has_command = nonempty_str(value.get("command")) or isinstance(value.get("command"), dict)
    has_exit = any(isinstance(value.get(key), int) for key in ["exit_status", "observed_exit_status"])
    has_tag = nonempty_str(value.get("resolved_tag")) or nonempty_str(value.get("release_tag"))
    has_substrate = nonempty_str(value.get("substrate")) or isinstance(value.get("substrate"), dict)
    has_stream_or_log = any(key in value for key in ["stdout", "stderr", "log_path", "logs"])
    return has_command and has_exit and has_tag and has_substrate and has_stream_or_log


def verify_text_proof_artifact(issue_id: str, path: Path, text: str) -> list[str]:
    lowered = text.lower()
    required = ["command", "exit", "substrate"]
    missing = [token for token in required if token not in lowered]
    if "resolved_tag" not in lowered and "resolved tag" not in lowered:
        missing.append("resolved_tag")
    if "stdout" not in lowered and "stderr" not in lowered and "log" not in lowered:
        missing.append("stdout/stderr or log")
    if missing:
        return [f"{issue_id}: verified text artifact {path} missing {', '.join(missing)}"]
    return []


def nonempty_str(value: object) -> bool:
    return isinstance(value, str) and bool(value)


if __name__ == "__main__":
    raise SystemExit(main())
