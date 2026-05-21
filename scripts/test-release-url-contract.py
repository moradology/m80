#!/usr/bin/env python3
"""Regression tests for public release URL source-of-truth drift."""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import tempfile
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
from quickstart_snippets import (
    expected_quickstart_snippets,
    extract_marked_quickstart_snippets,
    public_command_inventory,
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

QUICKSTART_SNIPPET_DOCS = [
    "README.md",
    "docs/runbook/release.md",
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

    def test_snippet_renderer_matches_public_install_commands(self) -> None:
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

    def test_marked_quickstart_snippets_match_shared_contract(self) -> None:
        expected = expected_quickstart_snippets()

        for relative in QUICKSTART_SNIPPET_DOCS:
            snippets = extract_marked_quickstart_snippets(REPO_ROOT / relative)
            self.assertEqual(
                snippets,
                expected,
                f"{relative} quickstart snippets diverged from the shared contract",
            )

    def test_public_command_inventory_classifies_every_public_command_snippet(self) -> None:
        expected_snippets = expected_quickstart_snippets()
        inventory = public_command_inventory(REPO_ROOT)
        observed = {(str(snippet.path), snippet.body, snippet.classification) for snippet in inventory}

        for expected in [
            ("README.md", expected_snippets["post-install-smoke"], "common"),
            ("README.md", expected_snippets["latest-install"], "common"),
            ("README.md", expected_snippets["pinned-install"], "pinned"),
            ("README.md", expected_snippets["verified-install-handoff"], "verified/operator"),
            ("README.md", "m80 install-status", "troubleshooting"),
            ("docs/runbook/release.md", expected_snippets["latest-install"], "common"),
            ("docs/runbook/release.md", expected_snippets["pinned-install"], "pinned"),
            ("docs/runbook/release.md", expected_snippets["verified-install-handoff"], "verified/operator"),
            (
                "docs/behaviors/release/legacy-quickstart-hard-cutover.md",
                "\n".join([expected_snippets["latest-install"], expected_snippets["pinned-install"]]),
                "legacy-internal",
            ),
        ]:
            self.assertIn(expected, observed)

    def test_latest_install_snippet_requires_public_access_proof_guard(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            readme = root / "README.md"
            readme.write_text(
                "\n".join(
                    [
                        "Fastest path on Linux:",
                        "",
                        "```sh",
                        latest_install_command(),
                        "```",
                    ]
                )
            )

            with self.assertRaisesRegex(ValueError, "public-access proof status note"):
                public_command_inventory(root)

            readme.write_text(
                "\n".join(
                    [
                        "<!-- m80:public-access-proof m80-o3uh9.21.7 pending -->",
                        "```sh",
                        latest_install_command(),
                        "```",
                    ]
                )
            )
            with self.assertRaisesRegex(ValueError, "public-access proof status note"):
                public_command_inventory(root)

    def test_public_command_inventory_rejects_stale_and_unclassified_commands(self) -> None:
        cases = [
            (
                "```sh\ncurl -fsSL https://raw.githubusercontent.com/moradology/m80/main/install.sh | sudo sh\n```\n",
                "mutable raw main",
            ),
            (
                "```sh\ncurl -fsSL https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz | tar xz\n```\n",
                "artifact-only latest URL",
            ),
            (
                "```sh\ncurl -fsSL https://github.com/example/m80/releases/latest/download/install.sh | sudo sh\n```\n",
                "expected moradology/m80",
            ),
            (
                "```sh\nm80 quickstart --artifact-url https://example.invalid/m80.tar.gz\n```\n",
                "unclassified public command snippet",
            ),
            (
                "  ```sh\n  m80 install --release-tag v1.2.3\n  ```\n",
                "unclassified public command snippet",
            ),
            (
                "  ```sh\n  curl -fsSL https://raw.githubusercontent.com/moradology/m80/main/install.sh | sudo sh\n  ```\n",
                "mutable raw main",
            ),
        ]

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc_dir = root / "docs" / "runbook"
            doc_dir.mkdir(parents=True)
            doc = doc_dir / "release.md"
            for body, expected_error in cases:
                doc.write_text(body)
                with self.assertRaisesRegex(ValueError, expected_error):
                    public_command_inventory(root)

    def test_public_command_inventory_allows_repair_scoped_pinned_troubleshooting(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc_dir = root / "docs" / "runbook"
            doc_dir.mkdir(parents=True)
            doc = doc_dir / "release.md"
            doc.write_text(
                "\n".join(
                    [
                        "Troubleshooting repair command for a broken active install:",
                        "",
                        "```sh",
                        "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh",
                        "```",
                    ]
                )
            )

            inventory = public_command_inventory(root)
            self.assertEqual(len(inventory), 1)
            self.assertEqual(inventory[0].classification, "troubleshooting")

    def test_public_command_inventory_classifies_indented_common_fences(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            readme = root / "README.md"
            readme.write_text(
                "\n".join(
                    [
                        "- install:",
                        "  public installer status is pending until the public-access proof is green",
                        "  <!-- m80:public-access-proof m80-o3uh9.21.7 pending -->",
                        "  ```sh",
                        f"  {latest_install_command()}",
                        "  ```",
                    ]
                )
            )

            inventory = public_command_inventory(root)
            self.assertEqual(len(inventory), 1)
            self.assertEqual(inventory[0].body, latest_install_command())
            self.assertEqual(inventory[0].classification, "common")

    def test_quickstart_snippet_marker_errors_are_actionable(self) -> None:
        cases = [
            (
                "<!-- m80:quickstart-snippet latest-install start -->\n```sh\nmissing end\n```\n",
                "missing end marker",
            ),
            (
                "<!-- m80:quickstart-snippet latest-install end -->\n",
                "unmatched quickstart snippet end",
            ),
            (
                "\n".join(
                    [
                        "<!-- m80:quickstart-snippet latest-install start -->",
                        "```sh",
                        "first",
                        "```",
                        "<!-- m80:quickstart-snippet latest-install end -->",
                        "<!-- m80:quickstart-snippet latest-install start -->",
                        "```sh",
                        "second",
                        "```",
                        "<!-- m80:quickstart-snippet latest-install end -->",
                    ]
                ),
                "duplicate quickstart snippet",
            ),
        ]

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "doc.md"
            for body, expected_error in cases:
                path.write_text(body)
                with self.assertRaisesRegex(ValueError, expected_error):
                    extract_marked_quickstart_snippets(path)

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
