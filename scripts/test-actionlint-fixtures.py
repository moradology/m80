#!/usr/bin/env python3
"""Live fixture tests for the pinned actionlint workflow syntax gate."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
RUNNER = REPO_ROOT / "scripts" / "run-actionlint.py"
FIXTURES = REPO_ROOT / "scripts" / "fixtures" / "actionlint"

spec = importlib.util.spec_from_file_location("run_actionlint", RUNNER)
assert spec is not None
runner = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules["run_actionlint"] = runner
spec.loader.exec_module(runner)


class ActionlintFixtureTest(unittest.TestCase):
    def test_valid_workflow_fixture_passes(self) -> None:
        self.assertEqual(run_fixture("valid"), 0)

    def test_invalid_expression_fixture_fails(self) -> None:
        self.assertNotEqual(run_fixture("invalid-expression"), 0)

    def test_invalid_event_syntax_fixture_fails(self) -> None:
        self.assertNotEqual(run_fixture("invalid-event-syntax"), 0)

    def test_stale_needs_fixture_fails(self) -> None:
        self.assertNotEqual(run_fixture("stale-needs"), 0)

    def test_duplicate_job_id_fixture_fails(self) -> None:
        self.assertNotEqual(run_fixture("duplicate-job-id"), 0)

    def test_concurrent_cli_invocations_share_cache_without_temp_collision(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            cache = Path(temp) / "cache"
            env = {**os.environ, "M80_ACTIONLINT_CACHE_DIR": str(cache)}
            commands = [
                [sys.executable, str(RUNNER), "--workflow-dir", str(FIXTURES / "valid")],
                [sys.executable, str(RUNNER), "--workflow-dir", str(REPO_ROOT / ".github" / "workflows")],
            ]
            processes = [
                subprocess.Popen(command, cwd=REPO_ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                for command in commands
            ]
            results = [process.communicate(timeout=60) for process in processes]
            return_codes = [process.returncode for process in processes]

            self.assertEqual(return_codes, [0, 0], format_process_results(commands, return_codes, results))
            self.assertFalse(list(cache.glob("**/.actionlint.*.tmp")))


def run_fixture(name: str) -> int:
    return runner.run_actionlint(
        FIXTURES / name,
        runner.default_cache_dir(),
        runner.PIN,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def format_process_results(commands: list[list[str]], return_codes: list[int | None], results: list[tuple[bytes, bytes]]) -> str:
    lines = []
    for command, return_code, (stdout, stderr) in zip(commands, return_codes, results):
        lines.append(f"command: {' '.join(command)}")
        lines.append(f"return_code: {return_code}")
        lines.append(f"stdout:\n{stdout.decode(errors='replace')}")
        lines.append(f"stderr:\n{stderr.decode(errors='replace')}")
    return "\n".join(lines)


if __name__ == "__main__":
    unittest.main()
