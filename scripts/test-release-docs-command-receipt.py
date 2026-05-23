#!/usr/bin/env python3
"""Tests for scripts/release_docs_command_receipt.py."""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "release_docs_command_receipt.py"


class ReleaseDocsCommandReceiptTests(unittest.TestCase):
    def test_clean_docs_receipt(self) -> None:
        with docs_fixture() as root:
            out = root / "receipt.json"

            result = run_receipt(root, out=out)

            self.assertEqual(result.returncode, 0, result.stderr)
            receipt = json.loads(out.read_text())
            self.assertEqual(receipt["lane_id"], "docs-command")
            self.assertEqual(receipt["status"], "passed")
            self.assertEqual(receipt["release_tag"], "v1.2.3")
            self.assertEqual(receipt["commit_sha"], "a" * 40)
            self.assertEqual(receipt["substrate"]["kind"], "github-actions")
            self.assertEqual(receipt["repository"], "moradology/m80")
            self.assertTrue(receipt["command_digest_sha256"].startswith("sha256:"))

    def test_stale_owner_repo_fails(self) -> None:
        with docs_fixture() as root:
            result = run_receipt(root, repository="example/m80")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("repository mismatch", result.stderr)

    def test_raw_main_url_fails(self) -> None:
        with docs_fixture() as root:
            readme = root / "README.md"
            readme.write_text(
                readme.read_text()
                + "\nhttps://raw.githubusercontent.com/moradology/m80/main/scripts/install.sh\n"
            )

            result = run_receipt(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("raw main install URL is forbidden", result.stderr)

    def test_missing_snippet_marker_fails(self) -> None:
        with docs_fixture() as root:
            readme = root / "README.md"
            readme.write_text(readme.read_text().replace("<!-- m80:quickstart-snippet latest-install start -->", ""))

            result = run_receipt(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unmatched quickstart snippet end", result.stderr)

    def test_generated_command_drift_fails(self) -> None:
        with docs_fixture() as root:
            fake_renderer = root / "bad-renderer.py"
            fake_renderer.write_text("#!/usr/bin/env python3\nprint('curl https://example.invalid/install.sh | sh')\n")

            result = run_receipt(root, renderer=fake_renderer)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("generated install snippet drift", result.stderr)

    def test_command_digest_mismatch_fails(self) -> None:
        with docs_fixture() as root:
            out = root / "receipt.json"
            result = run_receipt(root, out=out)
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(out.read_text())
            payload["command_digest_sha256"] = "sha256:" + ("0" * 64)
            out.write_text(json.dumps(payload) + "\n")

            verify = run_receipt(root, out=out, write=False)

            self.assertNotEqual(verify.returncode, 0)
            self.assertIn("command digest mismatch", verify.stderr)


def run_receipt(
    root: Path,
    *,
    out: Path | None = None,
    repository: str = "moradology/m80",
    renderer: Path | None = None,
    write: bool = True,
) -> subprocess.CompletedProcess[str]:
    args = [
        "python3",
        str(SCRIPT),
        "--root",
        str(root),
        "--repository",
        repository,
        "--release-tag",
        "v1.2.3",
        "--commit-sha",
        "a" * 40,
        "--workflow-run-id",
        "12345",
        "--out",
        str(out or (root / "receipt.json")),
    ]
    if renderer is not None:
        args.extend(["--renderer", str(renderer)])
    else:
        args.extend(["--renderer", str(REPO_ROOT / "scripts" / "render-release-install-snippets.py")])
    if write:
        args.append("--write")
    return subprocess.run(args, cwd=REPO_ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


class docs_fixture:
    def __enter__(self) -> Path:
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        (root / "docs" / "runbook").mkdir(parents=True)
        shutil.copy(REPO_ROOT / "README.md", root / "README.md")
        shutil.copy(REPO_ROOT / "docs" / "runbook" / "release.md", root / "docs" / "runbook" / "release.md")
        return root

    def __exit__(self, *args: object) -> None:
        self.tmp.cleanup()


if __name__ == "__main__":
    unittest.main(verbosity=2)
