#!/usr/bin/env python3
"""Unit tests for the E2E report schema validator."""

from __future__ import annotations

import importlib.util
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
VALIDATOR_PATH = ROOT / "scripts" / "validate-e2e-report.py"
spec = importlib.util.spec_from_file_location("validate_e2e_report", VALIDATOR_PATH)
assert spec and spec.loader
validate_e2e_report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validate_e2e_report)


def valid_report() -> dict:
    return {
        "schema_version": 3,
        "generated_at": "2026-05-26T00:00:00Z",
        "package": "m80-firecracker",
        "run_root": "/var/lib/m80-r",
        "list_only": True,
        "timeout_seconds": 180,
        "environment": {
            "sudo_available": True,
            "kvm_available": False,
            "artifacts_available": False,
            "firecracker_seccomp_filter_available": True,
            "host_binaries_manifest_available": True,
            "jailer_harden_available": True,
            "net_helper_available": True,
            "iproute2_available": True,
            "cgroup_v2_available": True,
            "loop_device_available": True,
            "debugfs_available": True,
            "mkfs_erofs_available": True,
            "docker_available": False,
            "measurement_enabled": False,
            "pmem_artifacts_available": False,
            "external_network_enabled": False,
            "malicious_artifacts_available": False,
            "minimal_artifacts_available": False,
            "ubuntu_artifacts_available": False,
        },
        "artifacts": {
            "artifact_dir": "/tmp/m80-build-current/artifacts",
            "kernel": "/tmp/m80-build-current/artifacts/vmlinux",
            "rootfs": "/tmp/m80-build-current/artifacts/output.ext4",
            "firecracker_seccomp_filter": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
            "host_binaries_manifest": "/tmp/m80-build-current/artifacts/host-binaries.manifest.json",
            "jailer_harden": "target/debug/m80-jailer-harden",
            "net_helper": "target/debug/m80-net-helper",
        },
        "summary": {
            "passed": 0,
            "failed": 1,
            "skipped": 1,
            "listed": 1,
            "total": 3,
        },
        "results": [
            {
                "status": "skip",
                "test": "needs_kvm",
                "binary": "example",
                "reason": "requires-kvm",
                "exit_code": 0,
                "duration_ms": 0,
            },
            {
                "status": "list",
                "test": "can_run",
                "binary": "example",
                "reason": None,
                "exit_code": 0,
                "duration_ms": 0,
            },
            {
                "status": "fail",
                "test": "times_out",
                "binary": "example",
                "reason": "timeout",
                "exit_code": 124,
                "duration_ms": 180000,
                "stdout_excerpt": "",
                "stderr_excerpt": "timeout",
            },
        ],
        "exit_code": 1,
    }


class E2EReportValidatorTests(unittest.TestCase):
    def assert_has_error(self, report: dict, expected: str) -> None:
        errors = validate_e2e_report.validate_report(report, source="fixture")
        self.assertTrue(errors)
        self.assertIn(expected, "\n".join(errors))

    def test_valid_report_passes(self) -> None:
        self.assertEqual(validate_e2e_report.validate_report(valid_report()), [])

    def test_unknown_top_level_field_fails_closed(self) -> None:
        report = valid_report()
        report["future"] = True
        self.assert_has_error(report, "unknown field(s): future")

    def test_integer_fields_reject_booleans(self) -> None:
        report = valid_report()
        report["schema_version"] = True
        report["summary"]["failed"] = False
        report["results"][0]["duration_ms"] = False
        errors = "\n".join(validate_e2e_report.validate_report(report, source="fixture"))
        self.assertIn("schema_version must be 3", errors)
        self.assertIn("summary.failed: must be a non-negative integer", errors)
        self.assertIn("results[0].duration_ms: must be a non-negative integer", errors)

    def test_summary_must_match_results(self) -> None:
        report = valid_report()
        report["summary"]["skipped"] = 99
        self.assert_has_error(report, "summary.skipped: expected 1")

    def test_skip_requires_reason(self) -> None:
        report = valid_report()
        report["results"][0]["reason"] = None
        self.assert_has_error(report, "skip results require a reason")

    def test_fail_requires_excerpts(self) -> None:
        report = valid_report()
        del report["results"][2]["stderr_excerpt"]
        self.assert_has_error(report, "fail results require a string")

    def test_success_exit_code_must_be_zero(self) -> None:
        report = valid_report()
        report["results"] = report["results"][:2]
        report["summary"] = {
            "passed": 0,
            "failed": 0,
            "skipped": 1,
            "listed": 1,
            "total": 2,
        }
        report["exit_code"] = 1
        self.assert_has_error(report, "exit_code: must be 0 when failed summary count is 0")


if __name__ == "__main__":
    unittest.main(verbosity=2)
