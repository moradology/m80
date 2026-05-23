#!/usr/bin/env python3
"""Audit GitHub repository protections required before release publication."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from typing import Any
from urllib.parse import urlparse


SCHEMA_VERSION = 1
KIND = "m80_repository_protection_audit"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, help="owner/repo")
    parser.add_argument("--branch", default="main")
    parser.add_argument("--tag-pattern", default="v*")
    parser.add_argument("--environment", default="m80-release-publish")
    parser.add_argument("--branch-protection-json", type=Path)
    parser.add_argument("--branch-rulesets-json", type=Path)
    parser.add_argument("--rulesets-json", type=Path)
    parser.add_argument("--environment-json", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--write", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    audit = build_audit(args)
    if args.write:
        args.out.write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n")
    else:
        current = read_json(args.out, "repository protection audit")
        if current != audit:
            print(f"{args.out}: repository protection audit is stale", file=sys.stderr)
            return 1
    if audit["status"] != "passed":
        for check in audit["checks"]:
            if check["status"] != "passed":
                print(
                    f"{check['id']}: {check['status']}: {check['remediation']}",
                    file=sys.stderr,
                )
        return 1
    print(f"repository protection audit ok: {args.out}")
    return 0


def build_audit(args: argparse.Namespace) -> dict[str, Any]:
    branch_payload = load_payload_or_gh(
        args.branch_protection_json,
        f"repos/{args.repository}/branches/{args.branch}/protection",
        "branch protection",
    )
    branch_rulesets_payload = load_payload_or_gh(
        args.branch_rulesets_json,
        f"repos/{args.repository}/rulesets?targets=branch",
        "branch rulesets",
    )
    rulesets_payload = load_payload_or_gh(
        args.rulesets_json,
        f"repos/{args.repository}/rulesets?targets=tag",
        "tag rulesets",
    )
    environment_payload = load_payload_or_gh(
        args.environment_json,
        f"repos/{args.repository}/environments/{args.environment}",
        "publish environment",
    )
    checks = [
        check_branch_required_checks(branch_payload, branch_rulesets_payload, args.branch),
        check_tag_ruleset(rulesets_payload, args.tag_pattern),
        check_environment(environment_payload, args.environment),
    ]
    status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
    if any(check["status"] == "unavailable" for check in checks):
        status = "unavailable"
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "repository": args.repository,
        "branch": args.branch,
        "tag_pattern": args.tag_pattern,
        "environment": args.environment,
        "status": status,
        "checks": checks,
    }


def load_payload_or_gh(path: Path | None, endpoint: str, label: str) -> dict[str, Any] | list[Any]:
    if path is not None:
        return read_json(path, label)
    result = subprocess.run(
        ["gh", "api", endpoint],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        return {
            "m80_api_error": {
                "endpoint": endpoint,
                "label": label,
                "stderr": result.stderr.strip(),
            }
        }
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        return {
            "m80_api_error": {
                "endpoint": endpoint,
                "label": label,
                "stderr": f"invalid JSON from gh api: {exc}",
            }
        }


def check_branch_required_checks(
    branch_payload: Any,
    branch_rulesets_payload: Any,
    branch: str,
) -> dict[str, Any]:
    contexts = required_status_contexts(branch_payload)
    observed: dict[str, Any] = {
        "branch_protection": branch_protection_observation(branch_payload, contexts),
        "branch_rulesets": [],
    }
    if contexts:
        return check(
            "main-branch-required-checks",
            "passed",
            f"{branch} branch protection or active branch ruleset with required status checks",
            observed,
            "none",
        )
    ruleset_unavailable = api_error(branch_rulesets_payload)
    if ruleset_unavailable is None:
        rulesets = hydrate_rulesets(
            branch_rulesets_payload if isinstance(branch_rulesets_payload, list) else []
        )
        matching = [
            ruleset_summary(ruleset)
            for ruleset in rulesets
            if isinstance(ruleset, dict) and ruleset_matches_branch_required_checks(ruleset, branch)
        ]
        observed["branch_rulesets"] = matching
        if matching:
            return check(
                "main-branch-required-checks",
                "passed",
                f"{branch} branch protection or active branch ruleset with required status checks",
                observed,
                "none",
            )

    protection_unavailable = api_error(branch_payload)
    if protection_unavailable is not None and ruleset_unavailable is not None:
        return check(
            "main-branch-required-checks",
            "unavailable",
            f"{branch} branch protection or active branch ruleset with required status checks",
            {
                "branch_protection": protection_unavailable,
                "branch_rulesets": ruleset_unavailable,
            },
            f"make GitHub branch rulesets readable and require status checks on {branch}",
        )
    if protection_unavailable is not None:
        return check(
            "main-branch-required-checks",
            "failed",
            f"{branch} branch protection or active branch ruleset with required status checks",
            observed,
            "add an active branch ruleset that covers "
            f"refs/heads/{branch} and requires status checks",
        )
    return check(
        "main-branch-required-checks",
        "failed",
        f"{branch} branch protection or active branch ruleset with required status checks",
        observed,
        f"configure required status checks on the {branch} branch protection "
        "rule or branch ruleset",
    )


def branch_protection_observation(payload: Any, contexts: list[str]) -> Any:
    unavailable = api_error(payload)
    if unavailable is not None:
        return unavailable
    return {"required_status_checks": contexts}


def required_status_contexts(payload: Any) -> list[str]:
    if not isinstance(payload, dict):
        return []
    status_checks = payload.get("required_status_checks")
    if not isinstance(status_checks, dict):
        return []
    contexts: list[str] = []
    raw_contexts = status_checks.get("contexts")
    if isinstance(raw_contexts, list):
        contexts.extend(str(value) for value in raw_contexts if value)
    raw_checks = status_checks.get("checks")
    if isinstance(raw_checks, list):
        for item in raw_checks:
            if isinstance(item, dict):
                name = item.get("context") or item.get("name")
                if name:
                    contexts.append(str(name))
    return sorted(set(contexts))


def ruleset_matches_branch_required_checks(ruleset: dict[str, Any], branch: str) -> bool:
    if ruleset.get("target") != "branch":
        return False
    if str(ruleset.get("enforcement", "")).lower() != "active":
        return False
    includes = ruleset_ref_includes(ruleset)
    expected = {branch, f"refs/heads/{branch}"}
    if not any(include in expected for include in includes):
        return False
    return bool(ruleset_required_status_contexts(ruleset))


def ruleset_required_status_contexts(ruleset: dict[str, Any]) -> list[str]:
    rules = ruleset.get("rules")
    if not isinstance(rules, list):
        return []
    contexts: list[str] = []
    for rule in rules:
        if not isinstance(rule, dict) or rule.get("type") != "required_status_checks":
            continue
        parameters = rule.get("parameters")
        if not isinstance(parameters, dict):
            continue
        required = parameters.get("required_status_checks")
        if not isinstance(required, list):
            continue
        for item in required:
            if isinstance(item, dict) and item.get("context"):
                contexts.append(str(item["context"]))
    return sorted(set(contexts))


def check_tag_ruleset(payload: Any, tag_pattern: str) -> dict[str, Any]:
    unavailable = api_error(payload)
    if unavailable is not None:
        return check(
            "release-tag-ruleset",
            "unavailable",
            f"active tag ruleset covering {tag_pattern}",
            unavailable,
            "make repository rulesets readable and add an active rule for release tags",
        )
    rulesets = hydrate_rulesets(payload if isinstance(payload, list) else [])
    matching = [
        ruleset_summary(ruleset)
        for ruleset in rulesets
        if isinstance(ruleset, dict) and ruleset_matches_tag_pattern(ruleset, tag_pattern)
    ]
    if matching:
        return check(
            "release-tag-ruleset",
            "passed",
            f"active tag ruleset covering {tag_pattern}",
            matching,
            "none",
        )
    return check(
        "release-tag-ruleset",
        "failed",
        f"active tag ruleset covering {tag_pattern}",
        [ruleset_summary(ruleset) for ruleset in rulesets if isinstance(ruleset, dict)],
        f"add an active repository ruleset that covers refs/tags/{tag_pattern}",
    )


def ruleset_matches_tag_pattern(ruleset: dict[str, Any], tag_pattern: str) -> bool:
    if ruleset.get("target") != "tag":
        return False
    if str(ruleset.get("enforcement", "")).lower() != "active":
        return False
    includes = ruleset_ref_includes(ruleset)
    expected = {tag_pattern, f"refs/tags/{tag_pattern}"}
    return any(include in expected for include in includes)


def hydrate_rulesets(rulesets: list[Any]) -> list[Any]:
    return [hydrate_ruleset(ruleset) for ruleset in rulesets]


def hydrate_ruleset(ruleset: Any) -> Any:
    if not isinstance(ruleset, dict) or ruleset_ref_includes(ruleset):
        return ruleset
    endpoint = ruleset_detail_endpoint(ruleset)
    if endpoint is None:
        return ruleset
    detail = load_payload_or_gh(None, endpoint, f"tag ruleset {ruleset.get('id')}")
    return detail if isinstance(detail, dict) else ruleset


def ruleset_detail_endpoint(ruleset: dict[str, Any]) -> str | None:
    links = ruleset.get("_links")
    if not isinstance(links, dict):
        return None
    self_link = links.get("self")
    if not isinstance(self_link, dict):
        return None
    href = self_link.get("href")
    if not isinstance(href, str) or not href:
        return None
    parsed = urlparse(href)
    if parsed.netloc and parsed.netloc != "api.github.com":
        return None
    path = parsed.path.lstrip("/")
    if not path:
        return None
    return f"{path}?{parsed.query}" if parsed.query else path


def ruleset_ref_includes(ruleset: dict[str, Any]) -> list[str]:
    conditions = ruleset.get("conditions")
    if not isinstance(conditions, dict):
        return []
    ref_name = conditions.get("ref_name")
    if not isinstance(ref_name, dict):
        return []
    include = ref_name.get("include")
    if not isinstance(include, list):
        return []
    return [str(value) for value in include if value]


def ruleset_summary(ruleset: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": ruleset.get("id"),
        "name": ruleset.get("name"),
        "target": ruleset.get("target"),
        "enforcement": ruleset.get("enforcement"),
        "include": ruleset_ref_includes(ruleset),
        "required_status_checks": ruleset_required_status_contexts(ruleset),
    }


def check_environment(payload: Any, environment: str) -> dict[str, Any]:
    unavailable = api_error(payload)
    if unavailable is not None:
        return check(
            "publish-environment-approval",
            "unavailable",
            f"{environment} environment with required reviewers",
            unavailable,
            f"make the {environment} environment readable and require reviewers",
        )
    observed = environment_summary(payload)
    if observed["name"] == environment and observed["required_reviewers"] > 0:
        return check(
            "publish-environment-approval",
            "passed",
            f"{environment} environment with required reviewers",
            observed,
            "none",
        )
    return check(
        "publish-environment-approval",
        "failed",
        f"{environment} environment with required reviewers",
        observed,
        f"create the {environment} environment and configure required reviewers",
    )


def environment_summary(payload: Any) -> dict[str, Any]:
    if not isinstance(payload, dict):
        return {"name": None, "required_reviewers": 0, "protection_rules": []}
    rules = payload.get("protection_rules")
    if not isinstance(rules, list):
        rules = []
    reviewer_count = 0
    rule_summaries: list[dict[str, Any]] = []
    for rule in rules:
        if not isinstance(rule, dict):
            continue
        reviewers = rule.get("reviewers")
        if not isinstance(reviewers, list):
            reviewers = []
        if rule.get("type") == "required_reviewers":
            reviewer_count += len(reviewers)
        rule_summaries.append(
            {
                "type": rule.get("type"),
                "reviewer_count": len(reviewers),
                "prevent_self_review": rule.get("prevent_self_review"),
            }
        )
    return {
        "name": payload.get("name"),
        "required_reviewers": reviewer_count,
        "protection_rules": rule_summaries,
    }


def check(
    check_id: str,
    status: str,
    expected: str,
    observed: Any,
    remediation: str,
) -> dict[str, Any]:
    return {
        "id": check_id,
        "status": status,
        "expected": expected,
        "observed": observed,
        "remediation": remediation,
    }


def api_error(payload: Any) -> dict[str, Any] | None:
    if isinstance(payload, dict) and isinstance(payload.get("m80_api_error"), dict):
        return payload["m80_api_error"]
    return None


def read_json(path: Path, label: str) -> Any:
    try:
        return json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise SystemExit(f"{label} missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid {label} JSON: {exc}") from exc


if __name__ == "__main__":
    raise SystemExit(main())
