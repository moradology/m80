#!/usr/bin/env python3
"""Tests for scripts/repository_protection_audit.py."""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
AUDIT = REPO_ROOT / "scripts" / "repository_protection_audit.py"


class RepositoryProtectionAuditTest(unittest.TestCase):
    def test_protected_repository_passes(self) -> None:
        with fixtures() as paths:
            result = run_audit(paths)

            self.assertEqual(result.returncode, 0, result.stderr)
            audit = json.loads(paths.out.read_text())
            self.assertEqual(audit["status"], "passed")
            self.assertEqual(
                [check["status"] for check in audit["checks"]],
                ["passed", "passed", "passed"],
            )

    def test_missing_branch_required_checks_fails(self) -> None:
        with fixtures(branch={"required_status_checks": {"contexts": []}}) as paths:
            result = run_audit(paths)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("main-branch-required-checks: failed", result.stderr)
            audit = json.loads(paths.out.read_text())
            self.assertEqual(audit["status"], "failed")
            self.assertIn("configure required status checks", audit["checks"][0]["remediation"])

    def test_missing_release_tag_ruleset_fails(self) -> None:
        with fixtures(rulesets=[]) as paths:
            result = run_audit(paths)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release-tag-ruleset: failed", result.stderr)
            audit = json.loads(paths.out.read_text())
            self.assertEqual(audit["checks"][1]["observed"], [])

    def test_missing_environment_approval_fails(self) -> None:
        with fixtures(environment={"name": "m80-release-publish", "protection_rules": []}) as paths:
            result = run_audit(paths)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("publish-environment-approval: failed", result.stderr)
            audit = json.loads(paths.out.read_text())
            self.assertEqual(audit["checks"][2]["observed"]["required_reviewers"], 0)

    def test_unavailable_api_payload_fails_closed(self) -> None:
        with fixtures(
            rulesets={
                "m80_api_error": {
                    "endpoint": "repos/moradology/m80/rulesets?targets=tag",
                    "label": "tag rulesets",
                    "stderr": "HTTP 403",
                }
            }
        ) as paths:
            result = run_audit(paths)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release-tag-ruleset: unavailable", result.stderr)
            audit = json.loads(paths.out.read_text())
            self.assertEqual(audit["status"], "unavailable")


class FixturePaths:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.branch = root / "branch.json"
        self.rulesets = root / "rulesets.json"
        self.environment = root / "environment.json"
        self.out = root / "audit.json"


class fixtures:
    def __init__(
        self,
        *,
        branch: object | None = None,
        rulesets: object | None = None,
        environment: object | None = None,
    ) -> None:
        self.branch = protected_branch() if branch is None else branch
        self.rulesets = protected_rulesets() if rulesets is None else rulesets
        self.environment = protected_environment() if environment is None else environment
        self.tempdir: tempfile.TemporaryDirectory[str] | None = None

    def __enter__(self) -> FixturePaths:
        self.tempdir = tempfile.TemporaryDirectory()
        paths = FixturePaths(Path(self.tempdir.name))
        paths.branch.write_text(json.dumps(self.branch))
        paths.rulesets.write_text(json.dumps(self.rulesets))
        paths.environment.write_text(json.dumps(self.environment))
        return paths

    def __exit__(self, *args: object) -> None:
        assert self.tempdir is not None
        self.tempdir.cleanup()


def run_audit(paths: FixturePaths) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            str(AUDIT),
            "--repository",
            "moradology/m80",
            "--branch-protection-json",
            str(paths.branch),
            "--rulesets-json",
            str(paths.rulesets),
            "--environment-json",
            str(paths.environment),
            "--out",
            str(paths.out),
            "--write",
        ],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def protected_branch() -> dict[str, object]:
    return {
        "required_status_checks": {
            "strict": True,
            "contexts": ["CI / test"],
        },
        "required_pull_request_reviews": {"required_approving_review_count": 1},
    }


def protected_rulesets() -> list[dict[str, object]]:
    return [
        {
            "id": 17,
            "name": "release tags",
            "target": "tag",
            "enforcement": "active",
            "conditions": {"ref_name": {"include": ["refs/tags/v*"], "exclude": []}},
        }
    ]


def protected_environment() -> dict[str, object]:
    return {
        "name": "m80-release-publish",
        "protection_rules": [
            {
                "type": "required_reviewers",
                "prevent_self_review": True,
                "reviewers": [{"type": "User", "reviewer": {"login": "moradology"}}],
            }
        ],
    }


if __name__ == "__main__":
    unittest.main()
