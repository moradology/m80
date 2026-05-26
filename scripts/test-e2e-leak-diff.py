#!/usr/bin/env python3
"""Unit tests for e2e-reap dry-run leak diffs."""

from __future__ import annotations

import importlib.util
import io
import pathlib
import tempfile
import unittest
from contextlib import redirect_stderr


ROOT = pathlib.Path(__file__).resolve().parents[1]
DIFF_PATH = ROOT / "scripts" / "e2e-leak-diff.py"
spec = importlib.util.spec_from_file_location("e2e_leak_diff", DIFF_PATH)
assert spec and spec.loader
e2e_leak_diff = importlib.util.module_from_spec(spec)
spec.loader.exec_module(e2e_leak_diff)


def report(*, actions: list[dict] | None = None, skipped: list[dict] | None = None) -> dict:
    return {
        "schema_version": 1,
        "dry_run": True,
        "actions": actions or [],
        "skipped": skipped or [],
        "errors": [],
    }


class E2ELeakDiffTests(unittest.TestCase):
    def test_unchanged_preexisting_residue_is_clean(self) -> None:
        residue = {
            "kind": "link",
            "target": "tfc1234abcd5678",
            "command": ["sudo", "-n", "ip", "link", "delete", "tfc1234abcd5678"],
        }

        diff = e2e_leak_diff.diff_report(
            report(actions=[residue]),
            report(actions=[residue]),
        )

        self.assertEqual(diff["new_resources"], [])

    def test_new_reaper_action_is_a_leak(self) -> None:
        diff = e2e_leak_diff.diff_report(
            report(),
            report(actions=[{"kind": "run-dir", "target": "/var/lib/m80-r/vm-leaked"}]),
        )

        self.assertEqual(
            diff["new_resources"],
            [
                {
                    "kind": "run-dir",
                    "target": "/var/lib/m80-r/vm-leaked",
                    "source": "actions",
                }
            ],
        )

    def test_new_skipped_live_resource_is_a_leak(self) -> None:
        diff = e2e_leak_diff.diff_report(
            report(),
            report(
                skipped=[
                    {
                        "kind": "cgroup",
                        "target": "/sys/fs/cgroup/m80-firecracker/vm-leaked",
                        "reason": "live pids",
                    }
                ]
            ),
        )

        self.assertEqual(diff["new_resources"][0]["source"], "skipped")
        self.assertEqual(diff["new_resources"][0]["reason"], "live pids")

    def test_cli_writes_report_and_returns_one_for_new_leak(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            tmp = pathlib.Path(raw)
            before = tmp / "before.json"
            after = tmp / "after.json"
            output = tmp / "diff.json"
            before.write_text('{"actions": [], "skipped": []}', encoding="utf-8")
            after.write_text(
                '{"actions": [{"kind": "link", "target": "tfc1234abcd5678"}], "skipped": []}',
                encoding="utf-8",
            )

            self.assertEqual(e2e_leak_diff.main([str(before), str(after), str(output)]), 1)
            self.assertIn("tfc1234abcd5678", output.read_text(encoding="utf-8"))

    def test_cli_returns_two_for_malformed_report(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            tmp = pathlib.Path(raw)
            before = tmp / "before.json"
            after = tmp / "after.json"
            output = tmp / "diff.json"
            before.write_text("[]", encoding="utf-8")
            after.write_text('{"actions": [], "skipped": []}', encoding="utf-8")

            stderr = io.StringIO()
            with redirect_stderr(stderr):
                status = e2e_leak_diff.main([str(before), str(after), str(output)])

            self.assertEqual(status, 2)
            self.assertIn("report must be a JSON object", stderr.getvalue())
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
