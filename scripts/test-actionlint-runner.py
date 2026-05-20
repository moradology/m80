#!/usr/bin/env python3
"""Tests for scripts/run-actionlint.py."""

from __future__ import annotations

import importlib.util
import io
import concurrent.futures
from pathlib import Path
import sys
import tarfile
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
RUNNER = REPO_ROOT / "scripts" / "run-actionlint.py"

spec = importlib.util.spec_from_file_location("run_actionlint", RUNNER)
assert spec is not None
runner = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules["run_actionlint"] = runner
spec.loader.exec_module(runner)


class ActionlintRunnerTest(unittest.TestCase):
    def test_verified_archive_installs_and_runs_pinned_actionlint(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            args_file = root / "args.txt"
            archive = make_actionlint_archive(
                root / "actionlint.tar.gz",
                fake_actionlint_script(args_file),
            )
            pin = pin_for_archive(archive)
            workflow_dir = root / "workflows"
            workflow_dir.mkdir()
            (workflow_dir / "ci.yml").write_text("name: CI\n")

            exit_code = runner.run_actionlint(workflow_dir, root / "cache", pin, host_target=pin.target)

            self.assertEqual(exit_code, 0)
            args = args_file.read_text().splitlines()
            self.assertEqual(args[0], "-no-color")
            self.assertEqual(args[1], str(workflow_dir / "ci.yml"))

    def test_missing_binary_cache_is_recreated_from_verified_archive(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = make_actionlint_archive(root / "actionlint.tar.gz", fake_actionlint_script(root / "args.txt"))
            pin = pin_for_archive(archive)
            cache_archive = root / "cache" / f"v{pin.version}" / pin.target / pin.archive_name
            cache_archive.parent.mkdir(parents=True)
            cache_archive.write_bytes(archive.read_bytes())

            binary = runner.ensure_actionlint(root / "cache", pin)

            self.assertTrue(binary.exists())
            self.assertTrue(binary.stat().st_mode & 0o111)

    def test_checksum_mismatch_fails_without_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = make_actionlint_archive(root / "actionlint.tar.gz", fake_actionlint_script(root / "args.txt"))
            pin = runner.ActionlintPin(
                version="0.0.0-test",
                target="linux_amd64",
                archive_name=archive.name,
                sha256="0" * 64,
                url=archive.as_uri(),
            )

            with self.assertRaisesRegex(runner.ActionlintRunnerError, "sha256 mismatch"):
                runner.ensure_actionlint(root / "cache", pin)

            self.assertFalse((root / "cache" / "v0.0.0-test" / "linux_amd64" / "actionlint").exists())

    def test_unsupported_platform_fails_closed(self) -> None:
        with self.assertRaisesRegex(runner.ActionlintRunnerError, "unsupported host platform"):
            runner.supported_target(system="Darwin", machine="x86_64")

    def test_download_failure_is_reported_without_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            missing = root / "missing.tar.gz"
            pin = runner.ActionlintPin(
                version="0.0.0-test",
                target="linux_amd64",
                archive_name=missing.name,
                sha256="0" * 64,
                url=missing.as_uri(),
            )

            with self.assertRaisesRegex(runner.ActionlintRunnerError, "failed to download pinned actionlint"):
                runner.ensure_actionlint(root / "cache", pin)

    def test_archive_without_actionlint_binary_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = make_archive(root / "actionlint.tar.gz", {"README.md": b"not a binary\n"})
            pin = pin_for_archive(archive)

            with self.assertRaisesRegex(runner.ActionlintRunnerError, "did not contain a file named actionlint"):
                runner.ensure_actionlint(root / "cache", pin)

    def test_concurrent_installs_do_not_share_temp_binary_path(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = make_actionlint_archive(
                root / "actionlint.tar.gz",
                fake_actionlint_script(root / "args.txt") + b"#" * (512 * 1024),
            )
            pin = pin_for_archive(archive)
            cache = root / "cache"

            with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                binaries = list(pool.map(lambda _: runner.ensure_actionlint(cache, pin), range(32)))

            self.assertEqual({path for path in binaries}, {cache / f"v{pin.version}" / pin.target / "actionlint"})
            self.assertTrue(binaries[0].exists())
            self.assertFalse(list(binaries[0].parent.glob(".actionlint.*.tmp")))


def fake_actionlint_script(args_file: Path) -> bytes:
    return textwrap.dedent(
        f"""\
        #!/bin/sh
        : > {args_file}
        for arg in "$@"; do
            printf '%s\\n' "$arg" >> {args_file}
        done
        exit 0
        """
    ).encode()


def pin_for_archive(path: Path) -> object:
    return runner.ActionlintPin(
        version="0.0.0-test",
        target="linux_amd64",
        archive_name=path.name,
        sha256=runner.sha256_file(path),
        url=path.as_uri(),
    )


def make_actionlint_archive(path: Path, actionlint: bytes) -> Path:
    return make_archive(path, {"actionlint": actionlint})


def make_archive(path: Path, files: dict[str, bytes]) -> Path:
    with tarfile.open(path, "w:gz") as tar:
        for name, content in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(content)
            info.mode = 0o755 if name == "actionlint" else 0o644
            tar.addfile(info, io.BytesIO(content))
    return path


if __name__ == "__main__":
    unittest.main()
