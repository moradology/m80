#!/usr/bin/env python3
"""Verify release/quickstart tracker proof-close policy."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any


DEFAULT_EPIC = "m80-o3uh9"
VERIFIED_LABEL = "requires-verified-close"
POLICY_EFFECTIVE_AT = datetime(2026, 5, 20, 18, 0, 0, tzinfo=timezone.utc)
VERIFIED_REF_RE = re.compile(r"verified:\s+([^\s]+)\s+@\s+([0-9a-fA-F]{7,40})\b")
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
    parser.add_argument("--epic", default=DEFAULT_EPIC, help="release epoch id to lint")
    parser.add_argument(
        "--skip-git-history",
        action="store_true",
        help="skip git commit/object checks for fixture-only invocations",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    errors = verify_tracker_policy(
        read_issues(args.issues),
        repo_root=repo_root,
        epic=args.epic,
        check_git_history=not args.skip_git_history,
    )
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
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
        errors.extend(verify_proof_artifact(issue_id, artifact))
    return errors


def artifact_path(repo_root: Path, rel_path: str) -> Path | None:
    path = Path(rel_path)
    if path.is_absolute() or ".." in path.parts:
        return None
    return repo_root / path


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


def verify_proof_artifact(issue_id: str, path: Path) -> list[str]:
    text = path.read_text(errors="replace")
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return verify_text_proof_artifact(issue_id, path, text)
    if not isinstance(value, dict):
        return [f"{issue_id}: verified artifact must be a JSON object or proof text: {path}"]
    if quickstart_proof_artifact_is_complete(value):
        return []
    if generic_proof_artifact_is_complete(value):
        return []
    return [
        f"{issue_id}: verified artifact must contain command, stdout/stderr or log path, "
        f"exit status, resolved tag, and substrate: {path}"
    ]


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
