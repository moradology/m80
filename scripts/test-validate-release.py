#!/usr/bin/env python3
"""Regression tests for validate-release.sh preflight failures."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / "scripts" / "validate-release.sh"


REQUIRED_ASSETS = (
    "install.sh",
    "m80-linux-x86_64.tar.gz",
    "m80-linux-x86_64.bundle.json",
    "m80-release-assets.json",
    "m80-bootstrap-selector.tsv",
    "m80-release-build.json",
    "m80-release-integrity.json",
    "m80-release-integrity.attestation.jsonl",
    "m80-release-attestation.json",
)


class ValidateReleasePrecheckTests(unittest.TestCase):
    def run_validate(self, dist: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "bash",
                str(SCRIPT),
                "--level",
                "a",
                "--tag",
                "v0.0.0",
                "--dist-dir",
                str(dist),
                "--skip-download",
                "--keep-work",
            ],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

    def write_dummy_dist(self, dist: Path) -> None:
        for asset in REQUIRED_ASSETS:
            payload = f"dummy {asset}\n".encode()
            (dist / asset).write_bytes(payload)

    def test_missing_asset_fails_before_verifier(self) -> None:
        with tempfile.TemporaryDirectory(dir="/tank/tmp") as tmp:
            dist = Path(tmp) / "dist"
            dist.mkdir()
            self.write_dummy_dist(dist)
            (dist / "install.sh").unlink()

            result = self.run_validate(dist)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing public release asset", result.stderr)
        self.assertIn("install.sh", result.stderr)

    def test_public_checksum_assets_are_not_required_before_verifier(self) -> None:
        with tempfile.TemporaryDirectory(dir="/tank/tmp") as tmp:
            dist = Path(tmp) / "dist"
            dist.mkdir()
            self.write_dummy_dist(dist)

            result = self.run_validate(dist)

        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("missing public release asset", result.stderr)
        self.assertNotIn(".sha256", result.stderr)
        self.assertNotIn("SHA256SUMS", result.stderr)


if __name__ == "__main__":
    unittest.main()
