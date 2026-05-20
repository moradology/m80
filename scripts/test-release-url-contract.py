#!/usr/bin/env python3
"""Regression tests for public release URL source-of-truth drift."""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import unittest

from release_url_contract import (
    CONTRACT_PATH,
    VERIFIED_INSTALL_HANDOFF_ASSETS,
    latest_install_command,
    pinned_install_command,
    public_release_root,
    release_asset_url,
    release_repository,
    verified_install_handoff_block,
)


REPO_ROOT = Path(__file__).resolve().parents[1]
INSTALL_URL_RE = re.compile(
    r"https://github\.com/([^/\s]+/[^/\s]+)/(?:releases/latest/download|releases/download/[^/\s]+)/install\.sh"
)
RAW_RELEASE_RE = re.compile(r"https://github\.com/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/releases/")
ARTIFACT_ONLY_LATEST_RE = re.compile(
    r"https://github\.com/[^/\s]+/[^/\s]+/releases/latest/download/[^`'\"\s]+\.tar\.gz"
)

PRODUCTION_URL_FILES = [
    "scripts/install.sh",
    "scripts/package-release-bundle.py",
    "scripts/stable_release_channel.py",
    "scripts/verify-release-bundle.py",
    "scripts/verify-release-integrity.py",
    "scripts/write-release-attestation-metadata.py",
    "crates/m80-cli/src/release_asset_index/fetch.rs",
    "crates/m80-cli/src/release_asset_index/diagnostics.rs",
    "crates/m80-cli/src/cmds/install/layout/source.rs",
]

QUICKSTART_SURFACE_FILES = [
    "README.md",
    "docs/runbook/release.md",
    "scripts/quickstart.sh",
    "crates/m80-cli/README.md",
    "docs/behaviors/cli/command-surface.md",
    "docs/behaviors/cli/product-surface.md",
    "docs/behaviors/release/legacy-quickstart-hard-cutover.md",
]


class ReleaseUrlContractTest(unittest.TestCase):
    def test_contract_is_moradology_m80(self) -> None:
        root = public_release_root()

        self.assertEqual(root.owner, "moradology")
        self.assertEqual(root.repo, "m80")
        self.assertEqual(root.repository, "moradology/m80")
        self.assertEqual(
            release_asset_url("v0.0.0", "install.sh"),
            "https://github.com/moradology/m80/releases/download/v0.0.0/install.sh",
        )

    def test_contract_renders_every_public_release_asset_url_shape(self) -> None:
        release_tag = "v1.2.3"
        expected_base = "https://github.com/moradology/m80/releases/download/v1.2.3"

        self.assertEqual(
            latest_install_command(),
            "curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh",
        )
        self.assertEqual(
            pinned_install_command(release_tag),
            f"curl -fsSL {expected_base}/install.sh | sudo sh",
        )
        self.assertIn(f"repo={release_repository()}", verified_install_handoff_block(release_tag))
        self.assertIn("scripts/verify-install-handoff.py", verified_install_handoff_block(release_tag))
        for asset_name in VERIFIED_INSTALL_HANDOFF_ASSETS:
            self.assertIn(asset_name, verified_install_handoff_block(release_tag))
        for asset_name in [
            "m80-release-assets.json",
            "install.sh",
            "m80-linux-x86_64.tar.gz",
            "m80-linux-x86_64.tar.gz.sha256",
            "m80-release-integrity.json",
        ]:
            self.assertEqual(
                release_asset_url(release_tag, asset_name),
                f"{expected_base}/{asset_name}",
            )

    def test_snippet_renderer_matches_readme_commands(self) -> None:
        output = subprocess.run(
            ["python3", "scripts/render-release-install-snippets.py"],
            cwd=REPO_ROOT,
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout.splitlines()

        self.assertEqual(
            output,
            [
                latest_install_command(),
                pinned_install_command(),
                "",
                *verified_install_handoff_block().splitlines(),
            ],
        )
        readme = read_repo_file("README.md")
        runbook = read_repo_file("docs/runbook/release.md")
        for command in [latest_install_command(), pinned_install_command()]:
            self.assertIn(command, readme)
            self.assertIn(command, runbook)
        self.assertIn(verified_install_handoff_block(), readme)
        self.assertIn(verified_install_handoff_block(), runbook)
        self.assertIn("scripts/render-release-install-snippets.py", readme)
        self.assertIn(str(CONTRACT_PATH.relative_to(REPO_ROOT)), runbook)

    def test_docs_use_only_the_public_release_repository_for_install_commands(self) -> None:
        for relative in ["README.md", "docs/runbook/release.md"]:
            text = read_repo_file(relative)
            install_urls = list(INSTALL_URL_RE.finditer(text))
            self.assertGreater(len(install_urls), 0, f"{relative} has no tested install URL")
            for match in install_urls:
                self.assertEqual(
                    match.group(1),
                    release_repository(),
                    f"{relative} uses the wrong public release repository",
                )
            self.assertNotIn("raw.githubusercontent.com", text)
            self.assertNotIn("/main/install.sh", text)

    def test_quickstart_surfaces_do_not_reintroduce_legacy_latest_artifacts(self) -> None:
        for relative in QUICKSTART_SURFACE_FILES:
            text = read_repo_file(relative)
            match = ARTIFACT_ONLY_LATEST_RE.search(text)
            self.assertIsNone(
                match,
                f"{relative} reintroduced artifact-only latest quickstart URL: {match.group(0) if match else ''}",
            )
            self.assertNotIn(
                "raw.githubusercontent.com",
                text,
                f"{relative} must not send users to mutable raw main installers",
            )
            self.assertNotIn(
                "/main/install.sh",
                text,
                f"{relative} must not send users to mutable main install.sh",
            )

    def test_production_code_does_not_hardcode_public_release_root(self) -> None:
        for relative in PRODUCTION_URL_FILES:
            text = read_repo_file(relative)
            matches = list(RAW_RELEASE_RE.finditer(text))
            self.assertEqual(
                matches,
                [],
                f"{relative} must derive GitHub release URLs from the shared contract",
            )

    def test_installer_template_uses_rendered_owner_repo_placeholders(self) -> None:
        text = read_repo_file("scripts/install.sh")

        self.assertIn("@M80_PUBLIC_RELEASE_OWNER@", text)
        self.assertIn("@M80_PUBLIC_RELEASE_REPO@", text)
        self.assertNotIn("github.com/moradology/m80/releases", text)


def read_repo_file(relative: str) -> str:
    return (REPO_ROOT / relative).read_text()


if __name__ == "__main__":
    unittest.main()
