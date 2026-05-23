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
    DEPRECATED_QUICKSTART_MARKER_RE,
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
QUICKSTART_VALUE_MARKER_RE = re.compile(
    r"^<!--\s*m80:quickstart-value\s+(start|end)\s*-->$"
)
QUICKSTART_VALUE_STATEMENT = (
    "`m80 run -- <command>` runs that process in a Firecracker microVM and returns stdout, stderr, and exit code."
)
QUICKSTART_VALUE_DOCS = [
    "README.md",
    "docs/behaviors/release/docs-quickstart-gate.md",
]
QUICKSTART_VALUE_FORBIDDEN_TERMS = [
    "artifact",
    "bundle",
    "guest manifest",
    "host binary",
    "receipt",
]

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
    "docs/ops/host-setup.md",
    "docs/ops/binary-installation.md",
    "docs/behaviors/cli/command-surface.md",
    "docs/behaviors/cli/product-surface.md",
    "docs/behaviors/release/quickstart-troubleshooting-matrix.md",
    "docs/behaviors/release/legacy-quickstart-hard-cutover.md",
]

QUICKSTART_SCRIPT_REFERENCE_ALLOWLIST = {
    "docs/runbook/release.md": "must not carry",
    "docs/behaviors/cli/command-surface.md": "only a thin launcher for that override path",
    "docs/behaviors/release/bundle-builder.md": "renderer rejects templates",
}

QUICKSTART_SNIPPET_DOCS = [
    "README.md",
    "docs/runbook/release.md",
]
README_QUICKSTART_LINKS = {
    "docs/behaviors/release/legacy-quickstart-hard-cutover.md",
    "docs/behaviors/release/quickstart-troubleshooting-matrix.md",
    "docs/behaviors/release/host-prerequisite-policy.md",
    "docs/behaviors/preflight/host-prerequisite-verifier.md",
    "docs/ops/host-setup.md",
}


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
        self.assertIn(verified_install_handoff_block(), runbook)
        self.assertIn(str(CONTRACT_PATH.relative_to(REPO_ROOT)), runbook)

    def test_marked_quickstart_snippets_match_shared_contract(self) -> None:
        expected = expected_quickstart_snippets()
        expected_by_doc = {
            "README.md": {
                "latest-install": expected["latest-install"],
                "post-install-smoke": expected["post-install-smoke"],
                "pinned-install": expected["pinned-install"],
            },
            "docs/runbook/release.md": expected,
        }

        for relative, expected_snippets in expected_by_doc.items():
            snippets = extract_marked_quickstart_snippets(REPO_ROOT / relative)
            self.assertEqual(
                snippets,
                expected_snippets,
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
            ("README.md", "m80 install-status", "troubleshooting"),
            (
                "docs/runbook/release.md",
                "\n".join(
                    [
                        "sudo ln -sfnT -- /opt/m80/versions/<previous-tag> /opt/m80/active",
                        "m80 install-status",
                        "sudo m80 install-cleanup --release-tag <old-tag>",
                    ]
                ),
                "troubleshooting",
            ),
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

            with self.assertRaisesRegex(ValueError, "freshness status note"):
                public_command_inventory(root)

            readme.write_text(
                "\n".join(
                    [
                        "<!-- m80:freshness-status start -->",
                        "Release channel template.",
                        "<!-- m80:freshness-status end -->",
                        "```sh",
                        latest_install_command(),
                        "```",
                    ]
                )
            )
            with self.assertRaisesRegex(ValueError, "freshness status note"):
                public_command_inventory(root)

    def test_quickstart_value_blocks_stay_process_focused(self) -> None:
        for relative in QUICKSTART_VALUE_DOCS:
            block = extract_quickstart_value_block(read_repo_file(relative), relative)
            self.assertEqual(block, QUICKSTART_VALUE_STATEMENT)
            lowered = block.lower()
            for term in QUICKSTART_VALUE_FORBIDDEN_TERMS:
                self.assertNotIn(
                    term,
                    lowered,
                    f"{relative} quickstart value block drifted into artifact management",
                )

        gate = read_repo_file("docs/behaviors/release/docs-quickstart-gate.md")
        self.assertIn(
            "The release proof observable for the public smoke is: `m80 run -- echo hello`",
            gate,
        )
        self.assertIn("exits 0 and stdout is exactly `hello`", gate)

    def test_readme_quickstart_stays_short_and_links_deep_details(self) -> None:
        quickstart = extract_markdown_section(read_repo_file("README.md"), "Quickstart")
        snippets = extract_marked_quickstart_snippets(REPO_ROOT / "README.md")
        expected = expected_quickstart_snippets()

        self.assertEqual(snippets["latest-install"], expected["latest-install"])
        self.assertEqual(snippets["post-install-smoke"], expected["post-install-smoke"])
        self.assertEqual(snippets["pinned-install"], expected["pinned-install"])
        self.assertNotIn("verified-install-handoff", snippets)
        self.assertIn(QUICKSTART_VALUE_STATEMENT, quickstart)

        for noisy_detail in [
            "m80-release-integrity",
            "m80-release-attestation",
            "SHA256SUMS",
            "guest manifest",
            "build receipt",
            "host binary versus guest artifact",
        ]:
            self.assertNotIn(noisy_detail, quickstart)

        readme_links = markdown_links(quickstart)
        for target in README_QUICKSTART_LINKS:
            self.assertTrue(
                link_targets_file(readme_links, target),
                f"README quickstart is missing follow-up doc link: {target}",
            )
            self.assertTrue((REPO_ROOT / target).is_file(), f"README quickstart link is stale: {target}")

    def test_ops_docs_name_installer_first_path_and_demote_manual_placement(self) -> None:
        expectations = {
            "docs/ops/host-setup.md": "## Normal Linux Install",
            "docs/ops/binary-installation.md": "## Advanced Manual Placement",
        }

        for relative, heading in expectations.items():
            text = read_repo_file(relative)
            normalized = " ".join(text.split())
            self.assertIn(latest_install_command(), text)
            self.assertIn("m80 run -- echo hello", text)
            self.assertIn(heading, text)
            self.assertIn(
                "Manual binary and artifact placement is an advanced/operator path",
                normalized,
                f"{relative} must keep manual placement out of the normal quickstart path",
            )

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
            (
                "<!-- m80:deprecated-quickstart start -->\n"
                "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz\n"
                "<!-- m80:deprecated-quickstart end -->\n",
                "artifact-only latest URL",
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

    def test_deprecated_quickstart_urls_are_allowed_only_in_migration_note_marker(self) -> None:
        inventory = public_command_inventory(REPO_ROOT)
        self.assertTrue(inventory)
        legacy_note = read_repo_file("docs/behaviors/release/legacy-quickstart-hard-cutover.md")
        self.assertIn("m80:deprecated-quickstart start", legacy_note)
        self.assertIn("releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz", legacy_note)
        self.assertIn("raw.githubusercontent.com/moradology/m80/main/scripts/install.sh", legacy_note)

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc = root / "docs" / "behaviors" / "release" / "legacy-quickstart-hard-cutover.md"
            doc.parent.mkdir(parents=True)
            doc.write_text(
                "\n".join(
                    [
                        "<!-- m80:deprecated-quickstart start -->",
                        "https://github.com/other/m80/releases/latest/download/install.sh",
                        "<!-- m80:deprecated-quickstart end -->",
                    ]
                )
            )
            with self.assertRaisesRegex(ValueError, "deprecated quickstart marker contains unclassified URL"):
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

    def test_public_command_inventory_allows_rollback_cleanup_troubleshooting(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            doc_dir = root / "docs" / "runbook"
            doc_dir.mkdir(parents=True)
            doc = doc_dir / "release.md"
            doc.write_text(
                "\n".join(
                    [
                        "Rollback and cleanup a broken active install:",
                        "",
                        "```sh",
                        "sudo ln -sfnT -- /opt/m80/versions/<previous-tag> /opt/m80/active",
                        "m80 install-status",
                        "sudo m80 install-cleanup --release-tag <old-tag>",
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
                        "  <!-- m80:freshness-status start -->",
                        "  Public installer status: pending public proof.",
                        "  <!-- m80:freshness-status end -->",
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
            self.assertNotIn("raw.githubusercontent.com", text)
            self.assertNotIn("/main/install.sh", text)
        for relative in QUICKSTART_SURFACE_FILES:
            text = read_repo_file(relative)
            for match in INSTALL_URL_RE.finditer(text):
                self.assertEqual(
                    match.group(1),
                    release_repository(),
                    f"{relative} uses the wrong public release repository",
                )

    def test_quickstart_surfaces_do_not_reintroduce_legacy_latest_artifacts(self) -> None:
        for relative in QUICKSTART_SURFACE_FILES:
            text = read_repo_file(relative)
            if relative == "docs/behaviors/release/legacy-quickstart-hard-cutover.md":
                text = DEPRECATED_QUICKSTART_MARKER_RE.sub("", text)
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

    def test_quickstart_script_reference_is_never_common_path(self) -> None:
        public_docs = [
            "README.md",
            "docs/runbook/release.md",
            "crates/m80-cli/README.md",
            "docs/behaviors/cli/command-surface.md",
            "docs/behaviors/cli/product-surface.md",
            "docs/behaviors/release/bundle-builder.md",
            "docs/behaviors/release/legacy-quickstart-hard-cutover.md",
        ]
        for relative in public_docs:
            text = read_repo_file(relative)
            if "scripts/quickstart.sh" not in text:
                continue
            required_context = QUICKSTART_SCRIPT_REFERENCE_ALLOWLIST.get(relative)
            self.assertIsNotNone(
                required_context,
                f"{relative} references scripts/quickstart.sh outside an override or forbidden-template context",
            )
            self.assertIn(
                required_context,
                text,
                f"{relative} references scripts/quickstart.sh without the required non-common-path context",
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


def extract_markdown_section(text: str, heading: str) -> str:
    start_marker = f"## {heading}"
    lines = text.splitlines()
    start = None
    for index, line in enumerate(lines):
        if line.strip() == start_marker:
            start = index + 1
            break
    if start is None:
        raise AssertionError(f"missing markdown section {heading!r}")
    end = len(lines)
    for index in range(start, len(lines)):
        if lines[index].startswith("## "):
            end = index
            break
    return "\n".join(lines[start:end]).strip()


def markdown_links(text: str) -> set[str]:
    return set(re.findall(r"\[[^\]]+\]\(([^)]+)\)", text))


def link_targets_file(links: set[str], target: str) -> bool:
    return target in links or any(link.startswith(f"{target}#") for link in links)


def extract_quickstart_value_block(text: str, relative: str) -> str:
    active = False
    block: list[str] = []
    seen = False

    for line_number, line in enumerate(text.splitlines(), start=1):
        marker = QUICKSTART_VALUE_MARKER_RE.fullmatch(line.strip())
        if marker is None:
            if active:
                block.append(line)
            continue

        kind = marker.group(1)
        if kind == "start":
            if active:
                raise AssertionError(f"{relative}:{line_number}: nested quickstart value block")
            if seen:
                raise AssertionError(f"{relative}:{line_number}: duplicate quickstart value block")
            active = True
            seen = True
            block = []
            continue

        if not active:
            raise AssertionError(f"{relative}:{line_number}: unmatched quickstart value end")
        active = False

    if active:
        raise AssertionError(f"{relative}: missing quickstart value end")
    if not seen:
        raise AssertionError(f"{relative}: missing quickstart value block")
    return "\n".join(line.strip() for line in block if line.strip())


if __name__ == "__main__":
    unittest.main()
