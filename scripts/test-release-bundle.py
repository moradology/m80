#!/usr/bin/env python3
"""Unit tests for scripts/package-release-bundle.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "package-release-bundle.py"
VERIFY = REPO_ROOT / "scripts" / "verify-release-bundle.py"
VERIFY_INTEGRITY = REPO_ROOT / "scripts" / "verify-release-integrity.py"
WRITE_ATTESTATION_METADATA = REPO_ROOT / "scripts" / "write-release-attestation-metadata.py"
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
ASSET_INDEX_NAME = "m80-release-assets.json"
BOOTSTRAP_SELECTOR_NAME = "m80-bootstrap-selector.tsv"
INSTALL_NAME = "install.sh"
INTEGRITY_NAME = "m80-release-integrity.json"
INTEGRITY_ATTESTATION_METADATA_NAME = "m80-release-attestation.json"
INTEGRITY_COMMIT_SHA = "0123456789abcdef0123456789abcdef01234567"
INTEGRITY_RUST_TOOLCHAIN = "1.82"
INTEGRITY_VERIFICATION_TIME = "2026-05-20T00:00:00Z"
INTEGRITY_KEYSET_ID = "github-actions-oidc:m80-release-v1"
INTEGRITY_SIGNER_IDENTITY = "moradology/m80/.github/workflows/release-artifacts.yml"
INTEGRITY_SIGNER_ISSUER = "https://token.actions.githubusercontent.com"
INTEGRITY_ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"


class ReleaseBundleTest(unittest.TestCase):
    def test_packages_release_bundle_with_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            out_dir = root / "out"

            run_package(inputs, out_dir)

            tarball = out_dir / BUNDLE_NAME
            run_verify(tarball, verify_sidecars=True)
            self.assertTrue(tarball.is_file())
            self.assertTrue((out_dir / f"{BUNDLE_NAME}.sha256").is_file())
            self.assertTrue((out_dir / INSTALL_NAME).is_file())
            self.assertTrue((out_dir / f"{INSTALL_NAME}.sha256").is_file())
            self.assertTrue((out_dir / METADATA_NAME).is_file())
            self.assertTrue((out_dir / f"{METADATA_NAME}.sha256").is_file())
            self.assertTrue((out_dir / ASSET_INDEX_NAME).is_file())
            self.assertTrue((out_dir / f"{ASSET_INDEX_NAME}.sha256").is_file())
            self.assertTrue((out_dir / BOOTSTRAP_SELECTOR_NAME).is_file())
            self.assertTrue((out_dir / f"{BOOTSTRAP_SELECTOR_NAME}.sha256").is_file())
            self.assertTrue((out_dir / "SHA256SUMS").is_file())
            self.assertTrue((out_dir / INTEGRITY_NAME).is_file())
            self.assertEqual(file_mode(tarball), 0o644)
            self.assertEqual(file_mode(out_dir / INSTALL_NAME), 0o755)
            self.assertEqual(file_mode(out_dir / f"{INSTALL_NAME}.sha256"), 0o644)
            self.assertEqual(file_mode(out_dir / METADATA_NAME), 0o644)
            self.assertEqual(file_mode(out_dir / f"{METADATA_NAME}.sha256"), 0o644)
            self.assertEqual(file_mode(out_dir / ASSET_INDEX_NAME), 0o644)
            self.assertEqual(file_mode(out_dir / f"{ASSET_INDEX_NAME}.sha256"), 0o644)
            self.assertEqual(file_mode(out_dir / BOOTSTRAP_SELECTOR_NAME), 0o644)
            self.assertEqual(file_mode(out_dir / f"{BOOTSTRAP_SELECTOR_NAME}.sha256"), 0o644)
            self.assertEqual(file_mode(out_dir / "SHA256SUMS"), 0o644)
            self.assertEqual(file_mode(out_dir / INTEGRITY_NAME), 0o644)
            with tarfile.open(tarball, "r:gz") as tar:
                names = set(tar.getnames())
                self.assertIn("bundle.json", names)
                self.assertIn("SHA256SUMS", names)
                self.assertIn("bin/m80", names)
                self.assertIn("artifacts/output.ext4.manifest.json", names)
                metadata = json.load(tar.extractfile("bundle.json"))  # type: ignore[arg-type]
                bundled_install = tar.extractfile("install.sh").read().decode("utf-8")  # type: ignore[union-attr]
                modes = {
                    member.name: member.mode & 0o777
                    for member in tar.getmembers()
                    if member.isfile()
                }

            self.assertEqual(metadata["release_tag"], "v0.0.0")
            self.assertEqual(metadata["m80_version"], "v0.0.0")
            self.assertEqual(metadata["package_version"], "0.0.0")
            self.assertEqual(metadata["target"], "linux-x86_64")
            self.assertEqual(metadata["os"], "linux")
            self.assertEqual(metadata["arch"], "x86_64")
            self.assertEqual(metadata["image_kind"], "minimal")
            self.assertEqual(metadata["m80_protocol_version"], 1)
            self.assertEqual(metadata["guestd_package_version"], "0.0.0")
            self.assertEqual(metadata["manifest_schema_version"], 5)
            self.assertEqual(metadata["build_receipt_schema_version"], 1)
            self.assertTrue(metadata["build_receipt_manifest_path"].endswith("output.ext4.manifest.json"))
            self.assertEqual(metadata["install_provenance_schema_version"], 1)
            self.assertEqual(metadata["install_provenance_required"], True)
            self.assertEqual(metadata["guest_protocol_version"], 1)
            self.assertEqual(metadata["expected_firecracker_version"], "v1.15.1")
            self.assertNotIn(
                "artifacts/host-binaries.manifest.json",
                {row["path"] for row in metadata["files"]},
            )
            self.assertEqual(modes["bin/m80"], 0o755)
            self.assertEqual(modes["install.sh"], 0o755)
            self.assertEqual(modes["artifacts/output.ext4"], 0o644)
            self.assertEqual(
                json.loads((out_dir / METADATA_NAME).read_text()),
                metadata,
            )
            public_install = (out_dir / INSTALL_NAME).read_text()
            self.assertEqual(public_install, bundled_install)
            self.assertIn("M80_RELEASE_TAG='v0.0.0'", public_install)
            self.assertIn("M80_ASSET_INDEX_NAME='m80-release-assets.json'", public_install)
            self.assertIn("M80_BOOTSTRAP_SELECTOR_NAME='m80-bootstrap-selector.tsv'", public_install)
            self.assertNotIn("M80_BUNDLE_URL=", public_install)
            self.assertNotIn("M80_BUNDLE_NAME=", public_install)
            self.assertIn("preflight_attestation_verifier", public_install)
            self.assertIn("gh attestation verify --help", public_install)
            self.assertIn("before downloading release assets", public_install)
            self.assertLess(
                public_install.index("\npreflight_attestation_verifier\n"),
                public_install.index('download_asset "$M80_BOOTSTRAP_SELECTOR_NAME"'),
            )
            self.assertIn('"$extract_dir/bin/m80" install --bundle-url "file://$bundle_path"', public_install)
            self.assertNotIn('\nexec "$extract_dir/bin/m80"', public_install)
            self.assertNotIn("@M80_", public_install)
            self.assertNotIn("m80 quickstart", public_install)
            self.assertNotIn("quickstart.sh", public_install)
            index = json.loads((out_dir / ASSET_INDEX_NAME).read_text())
            self.assertEqual(index["schema_version"], 1)
            self.assertEqual(index["release_tag"], "v0.0.0")
            self.assertEqual(len(index["assets"]), 1)
            asset = index["assets"][0]
            self.assertEqual(asset["name"], BUNDLE_NAME)
            self.assertEqual(
                asset["url"],
                f"https://github.com/moradology/m80/releases/download/v0.0.0/{BUNDLE_NAME}",
            )
            self.assertEqual(asset["sha256"], sha256(tarball))
            self.assertEqual(asset["size_bytes"], tarball.stat().st_size)
            self.assertEqual(asset["metadata_name"], METADATA_NAME)
            self.assertEqual(asset["metadata_sha256"], sha256(out_dir / METADATA_NAME))
            self.assertEqual(asset["checksum_name"], f"{BUNDLE_NAME}.sha256")
            self.assertIsNone(asset["signature_name"])
            self.assertEqual(asset["attestation_name"], INTEGRITY_ATTESTATION_BUNDLE_NAME)
            self.assertEqual(asset["target"], "linux-x86_64")
            self.assertEqual(asset["os"], "linux")
            self.assertEqual(asset["arch"], "x86_64")
            self.assertEqual(asset["image_kind"], "minimal")
            self.assertEqual(asset["m80_version"], "v0.0.0")
            self.assertEqual(asset["guest_protocol_version"], 1)
            self.assertEqual(asset["manifest_schema_version"], 5)
            self.assertEqual(asset["expected_firecracker_version"], "v1.15.1")
            selector = (out_dir / BOOTSTRAP_SELECTOR_NAME).read_text().splitlines()
            self.assertEqual(selector[0], "schema_version\t1")
            self.assertEqual(selector[1], "release_tag\tv0.0.0")
            self.assertEqual(
                selector[2],
                "columns\tos\tarch\timage_kind\tbundle_name\tbundle_url\tbundle_sha256\t"
                "size_bytes\tmetadata_name\tmetadata_sha256\tchecksum_name\tsignature_name\t"
                "attestation_name\tm80_version",
            )
            self.assertEqual(len(selector), 4)
            selector_row = selector[3].split("\t")
            self.assertEqual(selector_row[0], "row")
            self.assertEqual(selector_row[1:4], ["linux", "x86_64", "minimal"])
            self.assertEqual(selector_row[4], BUNDLE_NAME)
            self.assertEqual(selector_row[5], asset["url"])
            self.assertEqual(selector_row[6], asset["sha256"])
            self.assertEqual(selector_row[7], str(asset["size_bytes"]))
            self.assertEqual(selector_row[8], METADATA_NAME)
            self.assertEqual(selector_row[9], asset["metadata_sha256"])
            self.assertEqual(selector_row[10], f"{BUNDLE_NAME}.sha256")
            self.assertEqual(selector_row[11], "-")
            self.assertEqual(selector_row[12], INTEGRITY_ATTESTATION_BUNDLE_NAME)
            self.assertEqual(selector_row[13], "v0.0.0")
            integrity = json.loads((out_dir / INTEGRITY_NAME).read_text())
            self.assertEqual(integrity["schema_version"], 1)
            self.assertEqual(integrity["mechanism"], "github-artifact-attestation")
            self.assertEqual(integrity["repository"], "moradology/m80")
            self.assertEqual(integrity["release_tag"], "v0.0.0")
            self.assertEqual(integrity["commit_sha"], INTEGRITY_COMMIT_SHA)
            self.assertEqual(integrity["target"], "linux-x86_64")
            self.assertEqual(integrity["rust_toolchain"], INTEGRITY_RUST_TOOLCHAIN)
            self.assertEqual(integrity["m80_package_version"], "0.0.0")
            self.assertEqual(integrity["bundle_metadata_name"], METADATA_NAME)
            self.assertEqual(integrity["bundle_metadata_sha256"], sha256(out_dir / METADATA_NAME))
            self.assertEqual(
                {subject["name"] for subject in integrity["subjects"]},
                {
                    BUNDLE_NAME,
                    f"{BUNDLE_NAME}.sha256",
                    INSTALL_NAME,
                    f"{INSTALL_NAME}.sha256",
                    METADATA_NAME,
                    f"{METADATA_NAME}.sha256",
                    ASSET_INDEX_NAME,
                    f"{ASSET_INDEX_NAME}.sha256",
                    BOOTSTRAP_SELECTOR_NAME,
                    f"{BOOTSTRAP_SELECTOR_NAME}.sha256",
                    "SHA256SUMS",
                },
            )

    def test_package_rejects_legacy_quickstart_install_script(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            legacy = root / "legacy-quickstart.sh"
            write_executable(
                legacy,
                "#!/bin/sh\nexec \"${M80_BIN:-m80}\" quickstart --artifact-url \"$1\"\n",
            )
            inputs["install"] = legacy

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must use selector-driven m80 install", result.stderr)

    def test_package_rejects_hardcoded_public_bundle_install_script(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            hardcoded = root / "hardcoded-install.sh"
            write_executable(
                hardcoded,
                "#!/bin/sh\n"
                "M80_RELEASE_TAG='@M80_RELEASE_TAG@'\n"
                "curl -fsSL https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz -o bundle.tgz\n"
                "bin/m80 install --bundle-url file://bundle.tgz\n",
            )
            inputs["install"] = hardcoded

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must use selector-driven m80 install", result.stderr)

    def test_rendered_install_script_preflights_missing_gh_before_curl(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            fakebin = root / "fakebin"
            fakebin.mkdir()
            curl_marker = root / "curl-called"
            write_executable(
                fakebin / "curl",
                "#!/bin/sh\nprintf called > \"$M80_CURL_MARKER\"\nexit 88\n",
            )
            for tool in ["sha256sum", "tar", "mktemp", "chmod", "mkdir", "rm", "uname", "wc"]:
                write_executable(fakebin / tool, "#!/bin/sh\nexit 0\n")

            result = subprocess.run(
                [str(root / "out" / INSTALL_NAME)],
                check=False,
                text=True,
                capture_output=True,
                env={"PATH": str(fakebin), "M80_CURL_MARKER": str(curl_marker)},
            )

            self.assertEqual(result.returncode, 127)
            self.assertIn("release attestation verifier missing: gh", result.stderr)
            self.assertIn("before downloading release assets", result.stderr)
            self.assertFalse(curl_marker.exists(), "curl must not run before gh preflight")

    def test_rendered_install_script_selects_verified_selector_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)

            result, urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertEqual(result.returncode, 0, result.stderr)
            base = "https://github.com/moradology/m80/releases/download/v0.0.0"
            self.assertEqual(
                urls,
                [
                    f"{base}/{BOOTSTRAP_SELECTOR_NAME}",
                    f"{base}/{BOOTSTRAP_SELECTOR_NAME}.sha256",
                    f"{base}/{ASSET_INDEX_NAME}",
                    f"{base}/{ASSET_INDEX_NAME}.sha256",
                    f"{base}/{INTEGRITY_NAME}",
                    f"{base}/{INTEGRITY_ATTESTATION_BUNDLE_NAME}",
                    f"{base}/{INTEGRITY_ATTESTATION_METADATA_NAME}",
                    f"{base}/{METADATA_NAME}",
                    f"{base}/{METADATA_NAME}.sha256",
                    f"{base}/{INSTALL_NAME}",
                    f"{base}/{INSTALL_NAME}.sha256",
                    f"{base}/SHA256SUMS",
                    f"{base}/{BUNDLE_NAME}",
                    f"{base}/{BUNDLE_NAME}.sha256",
                ],
            )
            self.assertIn("verified release tag=v0.0.0", result.stderr)
            self.assertIn(
                f"verified assets={BUNDLE_NAME},{METADATA_NAME},{INSTALL_NAME},{INTEGRITY_NAME},{INTEGRITY_ATTESTATION_BUNDLE_NAME}",
                result.stderr,
            )
            self.assertIn(f"install_sh_sha256={sha256(root / 'out' / INSTALL_NAME)}", result.stderr)
            self.assertTrue(install_args.is_file(), result.stderr)
            args = install_args.read_text().splitlines()
            self.assertEqual(args[0:2], ["install", "--bundle-url"])
            self.assertTrue(args[2].startswith("file://"), args)
            self.assertTrue(args[2].endswith(f"/{BUNDLE_NAME}"), args)
            self.assertEqual(args[3:], ["--dry-run"])

    def test_rendered_install_script_rejects_unsigned_dev_fixture_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"failed to download {INTEGRITY_ATTESTATION_BUNDLE_NAME}", result.stderr)
            self.assertIn(
                "retry pinned command: curl -fsSL https://github.com/moradology/m80/releases/download/v0.0.0/install.sh | sudo sh",
                result.stderr,
            )
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())

    def test_rendered_install_script_rejects_tampered_install_before_bundle_extract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            with (root / "out" / INSTALL_NAME).open("a") as f:
                f.write("\n# tampered\n")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"checksum verification failed for {INSTALL_NAME}", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_wrong_integrity_tag_before_bundle_extract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            write_integrity_material(root / "out", updates={"release_tag": "v9.9.9"})
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity release_tag mismatch", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_failed_attestation_before_bundle_extract(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            rewrite_attestation_bundle(root / "out", {"valid": False})
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release attestation verification failed for {INTEGRITY_NAME}", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_unsupported_arch_before_downloads(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)

            result, urls, install_args = run_rendered_install(root, uname_arch="sparc")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported architecture for release install: sparc", result.stderr)
            self.assertEqual(urls, [])
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_selector_stale_tag_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            lines[1] = "release_tag\tv9.9.9"
            rewrite_bootstrap_selector(root / "out", lines)

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector release_tag mismatch", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_rendered_install_script_rejects_selector_stale_schema_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            lines[0] = "schema_version\t2"
            rewrite_bootstrap_selector(root / "out", lines)

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported bootstrap selector schema", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_rendered_install_script_rejects_selector_missing_tuple_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_bootstrap_selector(root / "out", bootstrap_selector_lines(root / "out")[:3])

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector missing tuple", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_rendered_install_script_rejects_wrong_image_kind_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            row = lines[3].split("\t")
            row[3] = "debug"
            lines[3] = "\t".join(row)
            rewrite_bootstrap_selector(root / "out", lines)

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("image_kind=minimal", result.stderr)
            self.assertIn("bootstrap selector missing tuple", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_rendered_install_script_rejects_selector_duplicate_tuple_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            lines.append(lines[3])
            rewrite_bootstrap_selector(root / "out", lines)

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector duplicate tuple", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_rendered_install_script_rejects_selector_checksum_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            (root / "out" / f"{BOOTSTRAP_SELECTOR_NAME}.sha256").write_text(
                f"{'0' * 64}  {BOOTSTRAP_SELECTOR_NAME}\n"
            )

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checksum verification failed for m80-bootstrap-selector.tsv", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_rendered_install_script_rejects_index_checksum_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            (root / "out" / f"{ASSET_INDEX_NAME}.sha256").write_text(
                f"{'0' * 64}  {ASSET_INDEX_NAME}\n"
            )

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checksum verification failed for m80-release-assets.json", result.stderr)
            assert_no_bundle_download(self, urls, install_args)

    def test_release_workflow_uses_versioned_install_template(self) -> None:
        workflow = (REPO_ROOT / ".github/workflows/release-artifacts.yml").read_text()

        self.assertIn("--install-sh scripts/install.sh", workflow)
        self.assertNotIn("--install-sh scripts/quickstart.sh", workflow)

    def test_ci_runs_release_script_tests(self) -> None:
        workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text()

        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/package-release-bundle.py")
        self.assertIn("python3 scripts/test-release-url-contract.py", workflow)
        self.assertIn("python3 scripts/test-release-bundle.py", workflow)
        self.assertIn('FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: "true"', workflow)
        self.assertIn("uses: actions/checkout@v6", workflow)
        self.assertIn("uses: actions/cache@v5", workflow)
        self.assertNotIn("actions/checkout@v4", workflow)
        self.assertNotIn("actions/cache@v4", workflow)
        self.assertIn("sudo apt-get install -y erofs-utils shellcheck", workflow)
        self.assertIn("cargo test -p m80-attack-runner --features malicious-artifact", workflow)
        self.assertIn(
            "cargo clippy -p m80-attack-runner --features malicious-artifact --all-targets -- -D warnings",
            workflow,
        )
        self.assertIn(
            '- name: cargo clippy --workspace --all-targets\n'
            '        env:\n'
            '          RUSTFLAGS: ""\n'
            '        run: cargo clippy --workspace --all-targets',
            workflow,
        )
        self.assertIn("RUSTFLAGS: \"\"", workflow)
        self.assertIn("rustup toolchain install 1.85 --profile minimal", workflow)
        self.assertIn("cargo +1.85 install cargo-audit --version 0.22.1 --locked", workflow)
        self.assertIn("cargo +1.85 audit", workflow)
        self.assertNotIn("rustsec/audit-check", workflow)

    def test_release_workflow_publishes_and_verifies_proof_assets(self) -> None:
        workflow = (REPO_ROOT / ".github/workflows/release-artifacts.yml").read_text()

        self.assertIn('FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: "true"', workflow)
        self.assertIn("uses: actions/checkout@v6", workflow)
        self.assertNotIn("actions/checkout@v4", workflow)
        self.assertIn("actions/attest@v4", workflow)
        self.assertIn("subject-name: m80-release-integrity.json", workflow)
        self.assertIn("subject-digest: ${{ steps.integrity-subject.outputs.digest }}", workflow)
        self.assertNotIn("subject-path:", workflow)
        self.assertIn("release_commit: ${{ steps.release-commit.outputs.sha }}", workflow)
        self.assertIn('echo "sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"', workflow)
        self.assertIn("RELEASE_COMMIT: ${{ steps.release-commit.outputs.sha }}", workflow)
        self.assertIn("RELEASE_COMMIT: ${{ needs.build-release-artifacts.outputs.release_commit }}", workflow)
        self.assertIn('--commit-sha "$RELEASE_COMMIT"', workflow)
        self.assertNotIn('--commit-sha "$GITHUB_SHA"', workflow)
        self.assertIn("id-token: write", workflow)
        self.assertIn("attestations: write", workflow)
        self.assertIn("scripts/write-release-attestation-metadata.py", workflow)
        self.assertGreaterEqual(workflow.count("scripts/verify-release-integrity.py"), 2)
        self.assertIn("scripts/stable_release_channel.py", workflow)
        self.assertIn('gh api "repos/${GITHUB_REPOSITORY}/releases/tags/${GITHUB_REF_NAME}"', workflow)
        self.assertIn("scripts/write-quickstart-proof-fixture.py", workflow)
        self.assertGreaterEqual(workflow.count("scripts/verify-quickstart-proof.py"), 2)
        self.assertIn("m80-quickstart-proof-hostless.json", workflow)
        self.assertIn("actions/upload-artifact", workflow)
        self.assertIn("actions/download-artifact", workflow)
        for name in [
            BUNDLE_NAME,
            f"{BUNDLE_NAME}.sha256",
            METADATA_NAME,
            f"{METADATA_NAME}.sha256",
            ASSET_INDEX_NAME,
            f"{ASSET_INDEX_NAME}.sha256",
            BOOTSTRAP_SELECTOR_NAME,
            f"{BOOTSTRAP_SELECTOR_NAME}.sha256",
            INSTALL_NAME,
            f"{INSTALL_NAME}.sha256",
            "SHA256SUMS",
        ]:
            self.assertGreaterEqual(
                workflow.count(f"/tmp/m80-release-upload/{name}"),
                1,
                f"{name} must be uploaded",
            )
            self.assertGreaterEqual(
                workflow.count(f"--pattern {name}"),
                1,
                f"{name} must be re-downloaded",
            )
        for name in [
            INTEGRITY_NAME,
            INTEGRITY_ATTESTATION_BUNDLE_NAME,
            INTEGRITY_ATTESTATION_METADATA_NAME,
        ]:
            self.assertGreaterEqual(
                workflow.count(f"/tmp/m80-release-upload/{name}"),
                1,
                f"{name} must be uploaded",
            )
            self.assertGreaterEqual(
                workflow.count(f"--pattern {name}"),
                1,
                f"{name} must be re-downloaded",
            )
            self.assertGreaterEqual(
                workflow.count(f"/tmp/m80-release-redownload/{name}"),
                1,
                f"{name} must be re-verified after publication",
            )

    def test_package_does_not_bundle_operator_host_prerequisites(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))

            with tarfile.open(tarball, "r:gz") as tar:
                names = set(tar.getnames())

            for forbidden in [
                "bin/firecracker",
                "bin/jailer",
                "bin/firecracker-seccomp-filter.bin",
                "artifacts/firecracker",
                "artifacts/jailer",
                "artifacts/firecracker-seccomp-filter.bin",
            ]:
                self.assertNotIn(forbidden, names)

    def test_package_tarball_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            first = root / "first"
            second = root / "second"

            run_package(inputs, first)
            run_package(inputs, second)

            self.assertEqual(
                sha256(first / "m80-linux-x86_64.tar.gz"),
                sha256(second / "m80-linux-x86_64.tar.gz"),
            )

    def test_verifier_rejects_missing_required_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "missing.tar.gz"
            rewrite_tar(tarball, broken, omit={"bin/m80"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle missing required paths", result.stderr)

    def test_verifier_rejects_duplicate_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "duplicate.tar.gz"
            rewrite_tar(tarball, broken, duplicate="bin/m80")

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle duplicate path", result.stderr)

    def test_verifier_rejects_unexpected_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "unexpected.tar.gz"
            rewrite_tar(tarball, broken, extra={"debug.txt": b"not part of the contract\n"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle contains unexpected paths", result.stderr)

    def test_verifier_rejects_bundled_operator_host_prerequisites(self) -> None:
        for forbidden in [
            "bin/firecracker",
            "bin/jailer",
            "bin/firecracker-seccomp-filter.bin",
            "artifacts/firecracker-seccomp-filter.json",
        ]:
            with self.subTest(forbidden=forbidden), tempfile.TemporaryDirectory() as tmp:
                tarball = package_fixture(Path(tmp))
                broken = Path(tmp) / f"{Path(forbidden).name}.tar.gz"
                rewrite_tar(
                    tarball,
                    broken,
                    extra={forbidden: b"operator-provided host prerequisite\n"},
                )

                result = run_verify(broken, check=False)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("operator-provided host prerequisite payloads", result.stderr)
                self.assertIn(forbidden, result.stderr)
                self.assertIn("m80 binaries/helpers and guest artifacts", result.stderr)

    def test_verifier_rejects_wrong_mode(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "wrong-mode.tar.gz"
            rewrite_tar(tarball, broken, mode_updates={"bin/m80": 0o644})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle mode mismatch for bin/m80", result.stderr)

    def test_verifier_rejects_wrong_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "wrong-target.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"target": "linux-arm64"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle target mismatch", result.stderr)

    def test_verifier_rejects_wrong_arch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "wrong-arch.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"arch": "arm64"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle arch mismatch", result.stderr)

    def test_verifier_rejects_wrong_image_kind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "wrong-image-kind.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"image_kind": "ubuntu"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle image_kind mismatch", result.stderr)

    def test_verifier_rejects_missing_install_provenance_requirement(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "missing-provenance-requirement.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"install_provenance_required": False})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle install_provenance_required missing", result.stderr)

    def test_verifier_rejects_stale_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "stale-version.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"m80_version": "v9.9.9"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle m80_version mismatch", result.stderr)

    def test_verifier_rejects_guestd_package_version_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "guestd-package-version.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"guestd_package_version": "9.9.9"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle guestd_package_version mismatch", result.stderr)

    def test_verifier_rejects_metadata_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "hash-mismatch.tar.gz"
            rewrite_tar(
                tarball,
                broken,
                metadata_file_updates={"bin/m80": "0" * 64},
            )

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle metadata hash mismatch", result.stderr)

    def test_rejects_binary_release_tag_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v9.9.9")
            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80 binary_version != release tag", result.stderr)

    def test_rejects_workspace_release_tag_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            result = run_package(inputs, root / "out", release_tag="v9.9.9", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release tag mismatch", result.stderr)

    def test_rejects_prerelease_release_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            result = run_package(inputs, root / "out", release_tag="v0.0.0-rc.1", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("stable release tag must be vMAJOR.MINOR.PATCH", result.stderr)

    def test_rejects_guest_protocol_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0", guest_protocol=2)
            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-guestd protocol_version != m80 protocol_version", result.stderr)

    def test_rejects_manifest_schema_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            rewrite_manifest(inputs["manifest"], {"schema_version": 99})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("guest manifest schema_version mismatch", result.stderr)

    def test_rejects_build_receipt_schema_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            rewrite_json(inputs["receipt"], {"schema_version": 99})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt schema_version mismatch", result.stderr)

    def test_rejects_guest_manifest_image_kind_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            rewrite_manifest(inputs["manifest"], {"image_kind": "ubuntu"})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("guest manifest image_kind mismatch", result.stderr)

    def test_rejects_unsupported_package_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")

            result = run_package(inputs, root / "out", target="linux-arm64", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported target", result.stderr)

    def test_rejects_unsupported_package_image_kind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")

            result = run_package(inputs, root / "out", image_kind="ubuntu", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported image kind", result.stderr)

    def test_rejects_build_receipt_manifest_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            rewrite_json(inputs["receipt"], {"manifest_sha256": "0" * 64})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt manifest_sha256 mismatch", result.stderr)

    def test_rejects_build_receipt_manifest_path_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            rewrite_json(inputs["receipt"], {"manifest_path": str(root / "other.manifest.json")})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt manifest_path mismatch", result.stderr)

    def test_rejects_build_receipt_artifact_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            receipt = json.loads(inputs["receipt"].read_text())
            receipt["artifacts"][0]["sha256"] = "0" * 64
            inputs["receipt"].write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt kernel_image sha256 mismatch", result.stderr)

    def test_verifier_rejects_build_receipt_manifest_path_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "receipt-path.tar.gz"
            rewrite_tar(
                tarball,
                broken,
                metadata_updates={"build_receipt_manifest_path": "/tmp/not-the-manifest.json"},
                payload_updates={
                    "artifacts/output.ext4.build-receipt.json": lambda data: rewrite_json_bytes(
                        data,
                        {"manifest_path": "/tmp/not-the-manifest.json"},
                    )
                },
            )

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt manifest_path mismatch", result.stderr)

    def test_verifier_rejects_build_receipt_manifest_path_metadata_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tarball = package_fixture(Path(tmp))
            broken = Path(tmp) / "receipt-path-metadata.tar.gz"
            rewrite_tar(tarball, broken, metadata_updates={"build_receipt_manifest_path": "/tmp/other.json"})

            result = run_verify(broken, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bundle build_receipt_manifest_path mismatch", result.stderr)

    def test_verifier_rejects_stale_public_checksum_sidecar(self) -> None:
        cases = [
            ("m80-linux-x86_64.tar.gz.sha256", "m80-linux-x86_64.tar.gz", "checksum sidecar mismatch"),
            ("install.sh.sha256", "install.sh", "checksum sidecar mismatch"),
            (
                f"{METADATA_NAME}.sha256",
                METADATA_NAME,
                "checksum sidecar mismatch",
            ),
            (f"{ASSET_INDEX_NAME}.sha256", ASSET_INDEX_NAME, "checksum sidecar mismatch"),
            (f"{BOOTSTRAP_SELECTOR_NAME}.sha256", BOOTSTRAP_SELECTOR_NAME, "checksum sidecar mismatch"),
            ("SHA256SUMS", INSTALL_NAME, f"public SHA256SUMS hash mismatch for {INSTALL_NAME}"),
        ]
        for sidecar, asset_name, expected_error in cases:
            with self.subTest(sidecar=sidecar), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                tarball = package_fixture(root)
                sidecar_path = root / "out" / sidecar
                if sidecar == "SHA256SUMS":
                    lines = []
                    for line in sidecar_path.read_text().splitlines():
                        if line.endswith(f"  {asset_name}"):
                            lines.append("0" * 64 + f"  {asset_name}")
                        else:
                            lines.append(line)
                    sidecar_path.write_text("\n".join(lines) + "\n")
                else:
                    sidecar_path.write_text("0" * 64 + f"  {asset_name}\n")

                result = run_verify(tarball, verify_sidecars=True, check=False)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected_error, result.stderr)

    def test_verifier_rejects_public_sidecar_wrong_mode(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            (root / "out" / "install.sh").chmod(0o644)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public sidecar mode mismatch for install.sh", result.stderr)

    def test_verifier_rejects_asset_index_missing_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            index_path = root / "out" / ASSET_INDEX_NAME
            index = json.loads(index_path.read_text())
            index["assets"] = []
            rewrite_asset_index(root / "out", index)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset index missing default bundle", result.stderr)

    def test_verifier_rejects_asset_index_wrong_hash(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"sha256": "0" * 64})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset index sha256 mismatch", result.stderr)

    def test_verifier_rejects_asset_index_wrong_tuple(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"target": "linux-aarch64", "arch": "aarch64"})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset index missing default bundle", result.stderr)

    def test_verifier_rejects_asset_index_duplicate_tuple(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            index_path = root / "out" / ASSET_INDEX_NAME
            index = json.loads(index_path.read_text())
            index["assets"].append(dict(index["assets"][0]))
            rewrite_asset_index(root / "out", index)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset index duplicate default bundle", result.stderr)

    def test_verifier_rejects_asset_index_missing_checksum_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            (root / "out" / f"{ASSET_INDEX_NAME}.sha256").unlink()

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"missing checksum sidecar: {ASSET_INDEX_NAME}.sha256", result.stderr)

    def test_verifier_rejects_asset_index_stale_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"m80_version": "v9.9.9"})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset index m80_version mismatch", result.stderr)

    def test_verifier_rejects_bootstrap_selector_unsupported_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            lines[0] = "schema_version\t999"
            rewrite_bootstrap_selector(root / "out", lines)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported bootstrap selector schema_version", result.stderr)

    def test_verifier_rejects_bootstrap_selector_stale_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            lines[1] = "release_tag\tv9.9.9"
            rewrite_bootstrap_selector(root / "out", lines)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector release_tag mismatch", result.stderr)

    def test_verifier_rejects_bootstrap_selector_missing_tuple(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")[:3]
            rewrite_bootstrap_selector(root / "out", lines)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector missing tuple", result.stderr)

    def test_verifier_rejects_bootstrap_selector_duplicate_tuple(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            lines.append(lines[3])
            rewrite_bootstrap_selector(root / "out", lines)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector duplicate tuple: linux/x86_64/minimal", result.stderr)

    def test_verifier_rejects_bootstrap_selector_stale_digest_or_size(self) -> None:
        cases = [
            (6, "0" * 64, "bootstrap selector bundle_sha256 mismatch"),
            (7, "999", "bootstrap selector size_bytes mismatch"),
        ]
        for column, value, expected_error in cases:
            with self.subTest(column=column), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                tarball = package_fixture(root)
                lines = bootstrap_selector_lines(root / "out")
                row = lines[3].split("\t")
                row[column] = value
                lines[3] = "\t".join(row)
                rewrite_bootstrap_selector(root / "out", lines)

                result = run_verify(tarball, verify_sidecars=True, check=False)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected_error, result.stderr)

    def test_verifier_rejects_hand_edited_bootstrap_selector_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            row = lines[3].split("\t")
            row[8] = "other.bundle.json"
            lines[3] = "\t".join(row)
            rewrite_bootstrap_selector(root / "out", lines)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector metadata_name mismatch", result.stderr)

    def test_verifier_rejects_bootstrap_selector_shell_metacharacters(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            row = lines[3].split("\t")
            row[5] = "https://example.invalid/$(id)"
            lines[3] = "\t".join(row)
            rewrite_bootstrap_selector(root / "out", lines)

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("bootstrap selector bundle_url contains non-shell-safe characters", result.stderr)

    def test_release_integrity_material_verifier_accepts_valid_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material)

            self.assertIn("verified release integrity material", result.stdout)

    def test_release_integrity_material_rejects_missing_asset_index_attestation_ref(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            remove_asset_index_asset_field(root / "out", "attestation_name")
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("asset index asset m80-linux-x86_64.tar.gz missing field(s): attestation_name", result.stderr)

    def test_release_integrity_material_rejects_empty_asset_index_attestation_ref(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"attestation_name": ""})
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index attestation_name missing", result.stderr)

    def test_release_integrity_material_rejects_stale_asset_index_attestation_ref(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"attestation_name": "old-release.attestation.jsonl"})
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index attestation_name mismatch", result.stderr)

    def test_release_integrity_material_rejects_attestation_bundle_path_name_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            renamed = root / "out" / "renamed.attestation.jsonl"
            renamed.write_text((root / "out" / INTEGRITY_ATTESTATION_BUNDLE_NAME).read_text())

            result = run_verify_integrity(material, attestation_bundle=renamed, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity attestation bundle path name mismatch", result.stderr)

    def test_release_integrity_material_rejects_stale_asset_index_release_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"release_tag": "v9.9.9"})
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index release_tag mismatch", result.stderr)

    def test_release_integrity_material_rejects_stale_asset_index_m80_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"m80_version": "v9.9.9"})
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index m80_version mismatch", result.stderr)

    def test_release_integrity_material_rejects_named_signature_without_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            signature = root / "out" / "m80-linux-x86_64.tar.gz.sig"
            signature.write_text("signature\n")
            rewrite_asset_index_asset(root / "out", {"signature_name": signature.name})
            rewrite_bootstrap_selector_asset_field(root / "out", "signature_name", signature.name)
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity missing subject(s): {signature.name}", result.stderr)

    def test_release_integrity_material_rejects_absent_asset_index_signature_ref(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"signature_name": "missing.sig"})
            rewrite_bootstrap_selector_asset_field(root / "out", "signature_name", "missing.sig")
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index signature_name missing file", result.stderr)

    def test_release_integrity_material_rejects_signature_name_colliding_with_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_asset_index_asset(root / "out", {"signature_name": BUNDLE_NAME})
            rewrite_bootstrap_selector_asset_field(root / "out", "signature_name", BUNDLE_NAME)
            material = write_integrity_material(
                root / "out",
                subject_updates={BUNDLE_NAME: {"kind": "detached-signature"}},
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index signature_name collides with required subject", result.stderr)

    def test_release_integrity_material_accepts_named_signature_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            signature = root / "out" / "m80-linux-x86_64.tar.gz.sig"
            signature.write_text("signature\n")
            rewrite_asset_index_asset(root / "out", {"signature_name": signature.name})
            rewrite_bootstrap_selector_asset_field(root / "out", "signature_name", signature.name)
            material = write_integrity_material(
                root / "out",
                extra_subject={
                    "name": signature.name,
                    "kind": "detached-signature",
                    "sha256": sha256(signature),
                    "size_bytes": signature.stat().st_size,
                },
            )

            result = run_verify_integrity(material)

            self.assertIn("verified release integrity material", result.stdout)

    def test_release_attestation_metadata_writer_accepts_verified_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = root / "out" / INTEGRITY_NAME
            write_trust_policy(root / "out")
            write_attestation_bundle(root / "out", material)
            write_fake_gh(root)

            metadata = root / "out" / INTEGRITY_ATTESTATION_METADATA_NAME
            result = run_write_attestation_metadata(material, metadata)

            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(metadata.read_text())
            self.assertEqual(payload["schema_version"], 1)
            self.assertEqual(payload["mechanism"], "github-artifact-attestation")
            self.assertEqual(payload["repository"], "moradology/m80")
            self.assertEqual(payload["release_tag"], "v0.0.0")
            self.assertEqual(payload["predicate_sha256"], sha256(material))
            self.assertEqual(payload["signer_identity"], INTEGRITY_SIGNER_IDENTITY)
            self.assertEqual(payload["issuer"], INTEGRITY_SIGNER_ISSUER)
            self.assertEqual(payload["keyset_id"], INTEGRITY_KEYSET_ID)
            self.assertEqual(payload["certificate_not_before"], "2026-01-01T00:00:00Z")
            self.assertEqual(payload["certificate_not_after"], "2027-01-01T00:00:00Z")
            run_verify_integrity(material)

    def test_release_integrity_material_preflights_missing_verifier_before_material_read(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = root / "out"
            out_dir.mkdir()

            result = run_verify_integrity(
                out_dir / "missing-material.json",
                gh_bin=root / "missing-gh",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier missing", result.stderr)
            self.assertIn(
                "Install or upgrade GitHub CLI with attestation support on Linux",
                result.stderr,
            )
            self.assertNotIn("release integrity material missing", result.stderr)

    def test_release_attestation_metadata_writer_preflights_missing_verifier_before_material_read(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = root / "out"
            out_dir.mkdir()

            result = run_write_attestation_metadata(
                out_dir / "missing-material.json",
                out_dir / INTEGRITY_ATTESTATION_METADATA_NAME,
                gh_bin=root / "missing-gh",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier missing", result.stderr)
            self.assertIn(
                "Install or upgrade GitHub CLI with attestation support on Linux",
                result.stderr,
            )
            self.assertNotIn("release integrity material missing", result.stderr)

    def test_release_integrity_material_rejects_too_old_attestation_verifier(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            write_fake_gh_without_attestation(root)

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier unsupported", result.stderr)
            self.assertIn("gh version 2.0.0", result.stderr)
            self.assertIn("unknown command \"attestation\"", result.stderr)
            self.assertIn(
                "Install or upgrade GitHub CLI with attestation support on Linux",
                result.stderr,
            )

    def test_release_integrity_material_rejects_attestation_verifier_missing_required_flag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            write_fake_gh_missing_help_flag(root, "--source-digest")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier unsupported", result.stderr)
            self.assertIn("--source-digest", result.stderr)
            self.assertIn("gh version 9.9.9", result.stderr)

    def test_release_integrity_material_accepts_complete_public_subject_set(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            payload = json.loads(material.read_text())

            self.assertEqual(
                {subject["name"] for subject in payload["subjects"]},
                {
                    BUNDLE_NAME,
                    f"{BUNDLE_NAME}.sha256",
                    INSTALL_NAME,
                    f"{INSTALL_NAME}.sha256",
                    METADATA_NAME,
                    f"{METADATA_NAME}.sha256",
                    ASSET_INDEX_NAME,
                    f"{ASSET_INDEX_NAME}.sha256",
                    BOOTSTRAP_SELECTOR_NAME,
                    f"{BOOTSTRAP_SELECTOR_NAME}.sha256",
                    "SHA256SUMS",
                },
            )
            run_verify_integrity(material)

    def test_release_integrity_material_rejects_wrong_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out", updates={"release_tag": "v9.9.9"})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity release_tag mismatch", result.stderr)

    def test_release_integrity_material_rejects_missing_asset_hash(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(
                root / "out",
                omit_subject_field=(BUNDLE_NAME, "sha256"),
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity subject {BUNDLE_NAME} missing field(s): sha256", result.stderr)

    def test_release_integrity_material_rejects_missing_install_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out", omit_subject=INSTALL_NAME)

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity missing subject(s): {INSTALL_NAME}", result.stderr)

    def test_release_integrity_material_rejects_missing_asset_index_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out", omit_subject=ASSET_INDEX_NAME)

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity missing subject(s): {ASSET_INDEX_NAME}", result.stderr)

    def test_release_integrity_material_rejects_missing_bootstrap_selector_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out", omit_subject=BOOTSTRAP_SELECTOR_NAME)

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity missing subject(s): {BOOTSTRAP_SELECTOR_NAME}", result.stderr)

    def test_release_integrity_material_rejects_unexpected_extra_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(
                root / "out",
                extra_subject={"name": "unpublished.txt", "kind": "extra", "sha256": "0" * 64, "size_bytes": 1},
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity unexpected subject unpublished.txt", result.stderr)

    def test_release_integrity_material_rejects_subject_digest_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(
                root / "out",
                subject_updates={INSTALL_NAME: {"sha256": "0" * 64}},
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity sha256 mismatch for {INSTALL_NAME}", result.stderr)

    def test_release_integrity_material_rejects_tampered_bundle_hash(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            with (root / "out" / BUNDLE_NAME).open("ab") as f:
                f.write(b"tampered")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity asset index sha256 mismatch for {BUNDLE_NAME}", result.stderr)

    def test_release_integrity_material_rejects_tampered_install_hash(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            with (root / "out" / INSTALL_NAME).open("a") as f:
                f.write("\n# tampered\n")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity sha256 mismatch for {INSTALL_NAME}", result.stderr)

    def test_release_integrity_material_rejects_signed_bootstrap_selector_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            row = lines[3].split("\t")
            row[7] = "999"
            lines[3] = "\t".join(row)
            rewrite_bootstrap_selector(root / "out", lines)
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity bootstrap selector size_bytes mismatch", result.stderr)

    def test_release_integrity_material_rejects_signed_bootstrap_selector_shell_metacharacters(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            lines = bootstrap_selector_lines(root / "out")
            row = lines[3].split("\t")
            row[5] = "https://example.invalid/$(id)"
            lines[3] = "\t".join(row)
            rewrite_bootstrap_selector(root / "out", lines)
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "release integrity bootstrap selector bundle_url contains non-shell-safe characters",
                result.stderr,
            )

    def test_release_integrity_material_rejects_unsupported_verifier_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out", updates={"schema_version": 999})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported release integrity schema_version", result.stderr)

    def test_release_integrity_material_rejects_missing_attestation_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            (root / "out" / "m80-release-attestation.json").unlink()

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation metadata missing", result.stderr)

    def test_release_integrity_material_rejects_missing_trust_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            trust_policy_path(root / "out").unlink()

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust policy missing", result.stderr)

    def test_release_integrity_material_rejects_missing_attestation_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            (root / "out" / INTEGRITY_ATTESTATION_BUNDLE_NAME).unlink()

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation bundle missing", result.stderr)

    def test_release_integrity_material_rejects_unsupported_trust_policy_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_trust_policy(root / "out", {"schema_version": 999})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported release trust policy schema_version", result.stderr)

    def test_release_integrity_material_rejects_unsupported_attestation_metadata_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_metadata(root / "out", {"schema_version": 999})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported release attestation metadata schema_version", result.stderr)

    def test_release_integrity_material_rejects_trust_policy_mechanism_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_trust_policy(root / "out", {"mechanism": "checksum-only"})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust mechanism mismatch", result.stderr)

    def test_release_integrity_material_rejects_failed_cryptographic_attestation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_bundle(root / "out", {"valid": False})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust cryptographic attestation verification failed", result.stderr)

    def test_release_integrity_material_rejects_attestation_without_material_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_bundle(root / "out", {"omit_subject": True})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier JSON omitted material name/sha256 subject", result.stderr)

    def test_release_integrity_material_rejects_attestation_subject_digest_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_bundle(root / "out", {"wrong_subject_digest": True})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier JSON omitted material name/sha256 subject", result.stderr)

    def test_release_integrity_material_rejects_attestation_subject_name_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_bundle(root / "out", {"wrong_subject_name": True})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation verifier JSON omitted material name/sha256 subject", result.stderr)

    def test_release_integrity_material_rejects_unknown_signer(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_metadata(
                root / "out",
                {
                    "signer_identity": "repo:moradology/other:ref:refs/tags/v0.0.0",
                },
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust signer not allowed", result.stderr)

    def test_release_integrity_material_rejects_stale_keyset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_metadata(
                root / "out",
                {"keyset_id": "github-actions-oidc:old"},
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust keyset mismatch", result.stderr)

    def test_release_integrity_material_rejects_expired_certificate_window(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_metadata(
                root / "out",
                {"certificate_not_after": "2026-05-19T23:59:59Z"},
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust certificate expired", result.stderr)

    def test_release_integrity_material_rejects_expired_trust_policy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_trust_policy(
                root / "out",
                {"valid_until": "2026-05-19T23:59:59Z"},
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust policy expired", result.stderr)

    def test_release_integrity_material_rejects_boolean_rotation_overlap(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_trust_policy(
                root / "out",
                {
                    "rotation": {
                        "mode": "hard-fail-expired",
                        "overlap_days": True,
                        "next_keyset_id": None,
                    }
                },
            )

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust rotation overlap_days invalid", result.stderr)

    def test_release_integrity_material_rejects_replayed_tag_attestation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_metadata(root / "out", {"release_tag": "v9.9.9"})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust attestation release_tag mismatch", result.stderr)

    def test_release_integrity_material_rejects_replayed_repo_attestation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out")
            rewrite_attestation_metadata(root / "out", {"repository": "moradology/other"})

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release trust attestation repository mismatch", result.stderr)


def fixture_inputs(root: Path, *, release_tag: str, guest_protocol: int = 1) -> dict[str, Path]:
    inputs = root / "inputs"
    inputs.mkdir()
    for name in [
        "m80-jailer-harden",
        "m80-net-helper",
        "vmlinux",
        "output.ext4",
    ]:
        write_executable(inputs / name, f"#!/bin/sh\nprintf '{name}\\n'\n")
    write_fake_m80(inputs / "m80", release_tag)
    write_fake_guestd(inputs / "m80-guestd", guest_protocol)
    write_guest_manifest(inputs)
    write_build_receipt(inputs)
    return {
        "m80": inputs / "m80",
        "jailer_harden": inputs / "m80-jailer-harden",
        "net_helper": inputs / "m80-net-helper",
        "kernel": inputs / "vmlinux",
        "rootfs": inputs / "output.ext4",
        "manifest": inputs / "output.ext4.manifest.json",
        "receipt": inputs / "output.ext4.build-receipt.json",
        "guestd": inputs / "m80-guestd",
        "install": REPO_ROOT / "scripts" / "install.sh",
    }


def write_fake_m80(path: Path, release_tag: str) -> None:
    payload = {
        "version": 1,
        "data": {
            "binary_version": release_tag,
            "package_version": "0.0.0",
            "release_tag": release_tag,
            "release_build": True,
            "version_status": "release",
            "expected_release_tag": "v0.0.0",
            "protocol_version": 1,
            "manifest_schema_version": 5,
            "build_receipt_schema_version": 1,
            "install_provenance_schema_version": 1,
            "firecracker_pin": "unknown",
        },
    }
    write_executable(
        path,
        "#!/bin/sh\n"
        "if [ \"${1:-}\" = install ]; then\n"
        "  if [ -n \"${M80_FAKE_INSTALL_ARGS:-}\" ]; then\n"
        "    : > \"$M80_FAKE_INSTALL_ARGS\"\n"
        "    for arg in \"$@\"; do printf '%s\\n' \"$arg\" >> \"$M80_FAKE_INSTALL_ARGS\"; done\n"
        "  fi\n"
        "  exit 0\n"
        "fi\n"
        f"cat <<'JSON'\n{json.dumps(payload)}\nJSON\n",
    )


def write_fake_guestd(path: Path, protocol_version: int) -> None:
    write_executable(path, f"#!/bin/sh\nprintf 'm80-guestd 0.0.0 (proto v{protocol_version})\\n'\n")


def write_guest_manifest(inputs: Path) -> None:
    manifest = {
        "daemon_binary_path": str(inputs / "m80-guestd"),
        "daemon_binary_sha256": sha256(inputs / "m80-guestd"),
        "expected_firecracker_version": "v1.15.1",
        "guest_port": 9001,
        "image_kind": "minimal",
        "kernel_image": str(inputs / "vmlinux"),
        "kernel_image_sha256": sha256(inputs / "vmlinux"),
        "kernel_kind": "stock",
        "no_egress_reason": None,
        "output_rootfs_image": str(inputs / "output.ext4"),
        "output_rootfs_sha256": sha256(inputs / "output.ext4"),
        "ready_marker": "GUESTD_READY",
        "rootfs_format": "ext4",
        "schema_version": 5,
        "source_rootfs_image": None,
        "source_rootfs_sha256": None,
    }
    (inputs / "output.ext4.manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def write_build_receipt(inputs: Path) -> None:
    manifest_path = inputs / "output.ext4.manifest.json"
    manifest = json.loads(manifest_path.read_text())
    receipt = {
        "artifacts": [
            {
                "kind": "kernel_image",
                "path": manifest["kernel_image"],
                "sha256": manifest["kernel_image_sha256"],
            },
            {
                "kind": "output_rootfs_image",
                "path": manifest["output_rootfs_image"],
                "sha256": manifest["output_rootfs_sha256"],
            },
            {
                "kind": "daemon_binary_path",
                "path": manifest["daemon_binary_path"],
                "sha256": manifest["daemon_binary_sha256"],
            },
        ],
        "manifest_path": str(manifest_path),
        "manifest_sha256": sha256(manifest_path),
        "schema_version": 1,
    }
    (inputs / "output.ext4.build-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")


def rewrite_manifest(path: Path, updates: dict) -> None:
    rewrite_json(path, updates)


def rewrite_json(path: Path, updates: dict) -> None:
    payload = json.loads(path.read_text())
    payload.update(updates)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def rewrite_json_bytes(data: bytes, updates: dict) -> bytes:
    payload = json.loads(data.decode("utf-8"))
    payload.update(updates)
    return (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode()


def rewrite_asset_index_asset(out_dir: Path, updates: dict) -> None:
    index_path = out_dir / ASSET_INDEX_NAME
    index = json.loads(index_path.read_text())
    index["assets"][0].update(updates)
    rewrite_asset_index(out_dir, index)


def remove_asset_index_asset_field(out_dir: Path, field: str) -> None:
    index_path = out_dir / ASSET_INDEX_NAME
    index = json.loads(index_path.read_text())
    index["assets"][0].pop(field, None)
    rewrite_asset_index(out_dir, index)


def rewrite_asset_index(out_dir: Path, index: dict) -> None:
    index_path = out_dir / ASSET_INDEX_NAME
    index_path.write_text(json.dumps(index, indent=2, sort_keys=True) + "\n")
    write_sha256_sidecar(out_dir / f"{ASSET_INDEX_NAME}.sha256", index_path, ASSET_INDEX_NAME)
    write_public_sha256s(
        out_dir / "SHA256SUMS",
        [
            (BUNDLE_NAME, out_dir / BUNDLE_NAME),
            (INSTALL_NAME, out_dir / INSTALL_NAME),
            (METADATA_NAME, out_dir / METADATA_NAME),
            (ASSET_INDEX_NAME, index_path),
            (BOOTSTRAP_SELECTOR_NAME, out_dir / BOOTSTRAP_SELECTOR_NAME),
        ],
    )


def bootstrap_selector_lines(out_dir: Path) -> list[str]:
    return (out_dir / BOOTSTRAP_SELECTOR_NAME).read_text().splitlines()


def rewrite_bootstrap_selector(out_dir: Path, lines: list[str]) -> None:
    selector = out_dir / BOOTSTRAP_SELECTOR_NAME
    selector.write_text("\n".join(lines) + "\n")
    write_sha256_sidecar(out_dir / f"{BOOTSTRAP_SELECTOR_NAME}.sha256", selector, BOOTSTRAP_SELECTOR_NAME)
    write_public_sha256s(
        out_dir / "SHA256SUMS",
        [
            (BUNDLE_NAME, out_dir / BUNDLE_NAME),
            (INSTALL_NAME, out_dir / INSTALL_NAME),
            (METADATA_NAME, out_dir / METADATA_NAME),
            (ASSET_INDEX_NAME, out_dir / ASSET_INDEX_NAME),
            (BOOTSTRAP_SELECTOR_NAME, selector),
        ],
    )


def rewrite_bootstrap_selector_asset_field(out_dir: Path, field: str, value: str | None) -> None:
    lines = bootstrap_selector_lines(out_dir)
    columns = lines[2].split("\t")
    row = lines[3].split("\t")
    row[columns.index(field)] = value if value is not None else "-"
    lines[3] = "\t".join(row)
    rewrite_bootstrap_selector(out_dir, lines)


def write_integrity_material(
    out_dir: Path,
    *,
    updates: dict | None = None,
    omit_subject: str | None = None,
    omit_subject_field: tuple[str, str] | None = None,
    extra_subject: dict | None = None,
    subject_updates: dict[str, dict] | None = None,
) -> Path:
    subject_kinds = {
        BUNDLE_NAME: "release-bundle",
        f"{BUNDLE_NAME}.sha256": "checksum-sidecar",
        INSTALL_NAME: "installer",
        f"{INSTALL_NAME}.sha256": "checksum-sidecar",
        METADATA_NAME: "bundle-metadata",
        f"{METADATA_NAME}.sha256": "checksum-sidecar",
        ASSET_INDEX_NAME: "asset-index",
        f"{ASSET_INDEX_NAME}.sha256": "checksum-sidecar",
        BOOTSTRAP_SELECTOR_NAME: "bootstrap-selector",
        f"{BOOTSTRAP_SELECTOR_NAME}.sha256": "checksum-sidecar",
        "SHA256SUMS": "checksum-manifest",
    }
    subjects = []
    for name, kind in subject_kinds.items():
        if omit_subject == name:
            continue
        asset = out_dir / name
        subject = {
            "name": name,
            "kind": kind,
            "sha256": sha256(asset),
            "size_bytes": asset.stat().st_size,
        }
        if subject_updates and name in subject_updates:
            subject.update(subject_updates[name])
        if omit_subject_field and omit_subject_field[0] == name:
            subject.pop(omit_subject_field[1], None)
        subjects.append(subject)
    if extra_subject:
        subjects.append(dict(extra_subject))
    payload = {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "release_tag": "v0.0.0",
        "commit_sha": INTEGRITY_COMMIT_SHA,
        "target": "linux-x86_64",
        "rust_toolchain": INTEGRITY_RUST_TOOLCHAIN,
        "m80_package_version": "0.0.0",
        "bundle_metadata_name": METADATA_NAME,
        "bundle_metadata_sha256": sha256(out_dir / METADATA_NAME),
        "subjects": subjects,
    }
    if updates:
        payload.update(updates)
    path = out_dir / "m80-release-integrity.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    write_trust_policy(out_dir)
    write_attestation_bundle(out_dir, path)
    write_attestation_metadata(out_dir, path)
    write_fake_gh(out_dir.parent)
    return path


def write_trust_policy(out_dir: Path, *, updates: dict | None = None) -> Path:
    payload = {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "keyset_id": INTEGRITY_KEYSET_ID,
        "valid_from": "2026-01-01T00:00:00Z",
        "valid_until": "2027-01-01T00:00:00Z",
        "allowed_signers": [
            {
                "identity": INTEGRITY_SIGNER_IDENTITY,
                "issuer": INTEGRITY_SIGNER_ISSUER,
            }
        ],
        "rotation": {
            "mode": "hard-fail-expired",
            "overlap_days": 14,
            "next_keyset_id": None,
        },
    }
    if updates:
        payload.update(updates)
    path = trust_policy_path(out_dir)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_attestation_bundle(out_dir: Path, material: Path, *, updates: dict | None = None) -> Path:
    payload = {
        "valid": True,
        "artifact": str(material),
        "repository": "moradology/m80",
        "release_tag": "v0.0.0",
        "commit_sha": INTEGRITY_COMMIT_SHA,
        "signer_identity": INTEGRITY_SIGNER_IDENTITY,
        "issuer": INTEGRITY_SIGNER_ISSUER,
    }
    if updates:
        payload.update(updates)
    path = out_dir / INTEGRITY_ATTESTATION_BUNDLE_NAME
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_attestation_metadata(out_dir: Path, material: Path, *, updates: dict | None = None) -> Path:
    payload = {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "release_tag": "v0.0.0",
        "predicate_sha256": sha256(material),
        "signer_identity": INTEGRITY_SIGNER_IDENTITY,
        "issuer": INTEGRITY_SIGNER_ISSUER,
        "keyset_id": INTEGRITY_KEYSET_ID,
        "certificate_not_before": "2026-01-01T00:00:00Z",
        "certificate_not_after": "2027-01-01T00:00:00Z",
    }
    if updates:
        payload.update(updates)
    path = out_dir / "m80-release-attestation.json"
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def rewrite_trust_policy(out_dir: Path, updates: dict) -> None:
    rewrite_json(trust_policy_path(out_dir), updates)


def rewrite_attestation_bundle(out_dir: Path, updates: dict) -> None:
    rewrite_json(out_dir / INTEGRITY_ATTESTATION_BUNDLE_NAME, updates)


def rewrite_attestation_metadata(out_dir: Path, updates: dict) -> None:
    rewrite_json(out_dir / "m80-release-attestation.json", updates)


def trust_policy_path(out_dir: Path) -> Path:
    return out_dir / "m80-release-trust-policy.json"


def fake_gh_path(root: Path) -> Path:
    return root / "fake-gh"


def write_fake_gh(root: Path) -> Path:
    expected = {
        "repo": "moradology/m80",
        "signer_workflow": INTEGRITY_SIGNER_IDENTITY,
        "cert_oidc_issuer": INTEGRITY_SIGNER_ISSUER,
        "source_ref": "refs/tags/v0.0.0",
        "source_digest": INTEGRITY_COMMIT_SHA,
    }
    script = f"""#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import sys

expected = {json.dumps(expected, sort_keys=True)}
help_text = " ".join([
    "--repo",
    "--bundle",
    "--signer-workflow",
    "--cert-oidc-issuer",
    "--source-ref",
    "--source-digest",
    "--deny-self-hosted-runners",
    "--format",
])

def fail(message):
    print(message, file=sys.stderr)
    sys.exit(1)

args = sys.argv[1:]
if args == ["--version"]:
    print("gh version 9.9.9")
    sys.exit(0)
if args == ["attestation", "verify", "--help"]:
    print(help_text)
    sys.exit(0)
if len(args) < 3 or args[:2] != ["attestation", "verify"]:
    fail("unexpected gh command")
artifact = args[2]
flags = {{}}
i = 3
while i < len(args):
    flag = args[i]
    if flag == "--deny-self-hosted-runners":
        flags[flag] = True
        i += 1
        continue
    if i + 1 >= len(args):
        fail("missing flag value")
    flags[flag] = args[i + 1]
    i += 2

for flag, key in [
    ("--repo", "repo"),
    ("--signer-workflow", "signer_workflow"),
    ("--cert-oidc-issuer", "cert_oidc_issuer"),
    ("--source-ref", "source_ref"),
    ("--source-digest", "source_digest"),
]:
    if flags.get(flag) != expected[key]:
        fail(f"{{flag}} mismatch: {{flags.get(flag)}}")
if flags.get("--format") != "json":
    fail("--format mismatch")
if flags.get("--deny-self-hosted-runners") is not True:
    fail("--deny-self-hosted-runners missing")
bundle_path = flags.get("--bundle")
if not bundle_path:
    fail("--bundle missing")
bundle = json.loads(Path(bundle_path).read_text())
if not bundle.get("valid", False):
    fail("cryptographic attestation invalid")
if bundle.get("artifact") != artifact:
    fail("artifact mismatch")
digest = hashlib.sha256(Path(artifact).read_bytes()).hexdigest()
if bundle.get("wrong_subject_digest", False):
    digest = "0" * 64
subject = {{"name": artifact, "digest": {{"sha256": digest}}}}
if bundle.get("wrong_subject_name", False):
    subject = {{"name": "other", "digest": {{"sha256": digest}}}}
subjects = [] if bundle.get("omit_subject", False) else [subject]
print(json.dumps([{{"verificationResult": {{"statement": {{"subject": subjects}}}}}}]))
"""
    path = fake_gh_path(root)
    path.write_text(script)
    path.chmod(0o755)
    return path


def write_fake_gh_without_attestation(root: Path) -> Path:
    path = fake_gh_path(root)
    write_executable(
        path,
        "#!/bin/sh\n"
        "if [ \"$1\" = \"--version\" ]; then printf 'gh version 2.0.0\\n'; exit 0; fi\n"
        "printf 'unknown command \"attestation\" for \"gh\"\\n' >&2\n"
        "exit 1\n",
    )
    return path


def write_fake_gh_missing_help_flag(root: Path, missing_flag: str) -> Path:
    flags = [
        "--repo",
        "--bundle",
        "--signer-workflow",
        "--cert-oidc-issuer",
        "--source-ref",
        "--source-digest",
        "--deny-self-hosted-runners",
        "--format",
    ]
    flags.remove(missing_flag)
    path = fake_gh_path(root)
    write_executable(
        path,
        "#!/bin/sh\n"
        "if [ \"$1\" = \"--version\" ]; then printf 'gh version 9.9.9\\n'; exit 0; fi\n"
        "if [ \"$1\" = \"attestation\" ] && [ \"$2\" = \"verify\" ] && [ \"$3\" = \"--help\" ]; then "
        f"printf '%s\\n' '{' '.join(flags)}'; exit 0; fi\n"
        "exit 1\n",
    )
    return path


def run_rendered_install(
    root: Path,
    *,
    args: list[str] | None = None,
    uname_arch: str = "x86_64",
) -> tuple[subprocess.CompletedProcess[str], list[str], Path]:
    out_dir = root / "out"
    fakebin = root / "fakebin-install"
    curl_log = root / "curl.log"
    tar_log = root / "tar.log"
    install_args = root / "install-args.log"
    write_install_test_tools(fakebin)
    env = {
        "PATH": str(fakebin),
        "M80_RELEASE_ROOT": str(out_dir),
        "M80_CURL_LOG": str(curl_log),
        "M80_TAR_LOG": str(tar_log),
        "M80_FAKE_UNAME_M": uname_arch,
        "M80_FAKE_INSTALL_ARGS": str(install_args),
    }
    result = subprocess.run(
        [str(out_dir / INSTALL_NAME), *(args or [])],
        check=False,
        text=True,
        capture_output=True,
        env=env,
    )
    urls = curl_log.read_text().splitlines() if curl_log.exists() else []
    return result, urls, install_args


def write_install_test_tools(fakebin: Path) -> None:
    fakebin.mkdir()
    write_executable(
        fakebin / "curl",
        "#!/bin/sh\n"
        "out=\n"
        "url=\n"
        "while [ \"$#\" -gt 0 ]; do\n"
        "  case \"$1\" in\n"
        "    -o) shift; out=$1 ;;\n"
        "    http://*|https://*) url=$1 ;;\n"
        "  esac\n"
        "  shift\n"
        "done\n"
        "[ -n \"$out\" ] || exit 2\n"
        "[ -n \"$url\" ] || exit 2\n"
        "printf '%s\\n' \"$url\" >> \"$M80_CURL_LOG\"\n"
        "name=${url##*/}\n"
        "src=$M80_RELEASE_ROOT/$name\n"
        "[ -f \"$src\" ] || exit 22\n"
        "/bin/cp \"$src\" \"$out\"\n",
    )
    write_executable(
        fakebin / "gh",
        f"""#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import sys

EXPECTED = {{
    "repo": "moradology/m80",
    "signer": "{INTEGRITY_SIGNER_IDENTITY}",
    "issuer": "{INTEGRITY_SIGNER_ISSUER}",
    "source_ref": "refs/tags/v0.0.0",
}}
HELP = "--repo --bundle --signer-workflow --cert-oidc-issuer --source-ref --source-digest --deny-self-hosted-runners --format"


def fail(message):
    print(message, file=sys.stderr)
    sys.exit(1)


args = sys.argv[1:]
if args == ["--version"]:
    print("gh version 9.9.9")
    sys.exit(0)
if args == ["attestation", "verify", "--help"]:
    print(HELP)
    sys.exit(0)
if len(args) < 3 or args[:2] != ["attestation", "verify"]:
    fail("unexpected gh command")
artifact = Path(args[2])
flags = {{}}
i = 3
while i < len(args):
    flag = args[i]
    if flag == "--deny-self-hosted-runners":
        flags[flag] = True
        i += 1
        continue
    if i + 1 >= len(args):
        fail(f"missing value for {{flag}}")
    flags[flag] = args[i + 1]
    i += 2
if flags.get("--repo") != EXPECTED["repo"]:
    fail("--repo mismatch")
if flags.get("--signer-workflow") != EXPECTED["signer"]:
    fail("--signer-workflow mismatch")
if flags.get("--cert-oidc-issuer") != EXPECTED["issuer"]:
    fail("--cert-oidc-issuer mismatch")
if flags.get("--source-ref") != EXPECTED["source_ref"]:
    fail("--source-ref mismatch")
if flags.get("--format") != "json":
    fail("--format mismatch")
if flags.get("--deny-self-hosted-runners") is not True:
    fail("--deny-self-hosted-runners missing")
bundle_path = flags.get("--bundle")
if not bundle_path:
    fail("--bundle missing")
bundle = json.loads(Path(bundle_path).read_text())
if not bundle.get("valid", False):
    fail("cryptographic attestation invalid")
material = json.loads(artifact.read_text())
if flags.get("--source-digest") != material["commit_sha"]:
    fail("--source-digest mismatch")
if bundle.get("repository") != EXPECTED["repo"]:
    fail("bundle repository mismatch")
if bundle.get("release_tag") != "v0.0.0":
    fail("bundle release_tag mismatch")
if bundle.get("commit_sha") != material["commit_sha"]:
    fail("bundle commit_sha mismatch")
if bundle.get("signer_identity") != EXPECTED["signer"]:
    fail("bundle signer mismatch")
if bundle.get("issuer") != EXPECTED["issuer"]:
    fail("bundle issuer mismatch")
digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
print(json.dumps([{{"verificationResult": {{"statement": {{"subject": [{{"name": str(artifact), "digest": {{"sha256": digest}}}}]}}}}}}]))
""",
    )
    write_executable(
        fakebin / "uname",
        "#!/bin/sh\n"
        "case \"$1\" in\n"
        "  -s) printf 'Linux\\n' ;;\n"
        "  -m) printf '%s\\n' \"$M80_FAKE_UNAME_M\" ;;\n"
        "  *) exit 1 ;;\n"
        "esac\n",
    )
    write_executable(
        fakebin / "mktemp",
        "#!/bin/sh\n"
        "[ \"$1\" = -d ] || exit 2\n"
        "base=${2%XXXXXX}\n"
        "dir=${base}fake\n"
        "i=0\n"
        "while [ -e \"$dir\" ]; do i=$((i + 1)); dir=${base}fake-$i; done\n"
        "/bin/mkdir -p \"$dir\"\n"
        "printf '%s\\n' \"$dir\"\n",
    )
    for name, target in [
        ("sha256sum", "/usr/bin/sha256sum"),
        ("python3", "/usr/bin/python3"),
        ("chmod", "/bin/chmod"),
        ("mkdir", "/bin/mkdir"),
        ("rm", "/bin/rm"),
        ("wc", "/usr/bin/wc"),
    ]:
        write_executable(fakebin / name, f"#!/bin/sh\nexec {target} \"$@\"\n")
    write_executable(
        fakebin / "tar",
        "#!/bin/sh\n"
        "if [ -n \"${M80_TAR_LOG:-}\" ]; then printf '%s\\n' \"$*\" >> \"$M80_TAR_LOG\"; fi\n"
        "PATH=/usr/bin:/bin exec /usr/bin/tar \"$@\"\n",
    )


def assert_no_bundle_download(test: unittest.TestCase, urls: list[str], install_args: Path) -> None:
    bundle_url = f"https://github.com/moradology/m80/releases/download/v0.0.0/{BUNDLE_NAME}"
    test.assertNotIn(bundle_url, urls)
    test.assertFalse(install_args.exists())


def write_executable(path: Path, text: str) -> None:
    path.write_text(text)
    path.chmod(0o755)


def run_package(
    inputs: dict[str, Path],
    out_dir: Path,
    *,
    release_tag: str = "v0.0.0",
    target: str = "linux-x86_64",
    image_kind: str = "minimal",
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(SCRIPT),
        "--repo-root",
        str(REPO_ROOT),
        "--release-tag",
        release_tag,
        "--commit-sha",
        INTEGRITY_COMMIT_SHA,
        "--rust-toolchain",
        INTEGRITY_RUST_TOOLCHAIN,
        "--target",
        target,
        "--image-kind",
        image_kind,
        "--m80-bin",
        str(inputs["m80"]),
        "--jailer-harden-bin",
        str(inputs["jailer_harden"]),
        "--net-helper-bin",
        str(inputs["net_helper"]),
        "--kernel",
        str(inputs["kernel"]),
        "--rootfs",
        str(inputs["rootfs"]),
        "--rootfs-manifest",
        str(inputs["manifest"]),
        "--build-receipt",
        str(inputs["receipt"]),
        "--guestd",
        str(inputs["guestd"]),
        "--install-sh",
        str(inputs["install"]),
        "--out-dir",
        str(out_dir),
    ]
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def package_fixture(root: Path) -> Path:
    inputs = fixture_inputs(root, release_tag="v0.0.0")
    out_dir = root / "out"
    run_package(inputs, out_dir)
    return out_dir / "m80-linux-x86_64.tar.gz"


def package_signed_fixture(root: Path) -> Path:
    tarball = package_fixture(root)
    write_integrity_material(root / "out")
    return tarball


def run_verify(
    tarball: Path,
    *,
    check: bool = True,
    verify_sidecars: bool = False,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(VERIFY),
        str(tarball),
        "--repo-root",
        str(REPO_ROOT),
        "--release-tag",
        "v0.0.0",
    ]
    if verify_sidecars:
        cmd.append("--verify-sidecars")
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def run_verify_integrity(
    material: Path,
    *,
    gh_bin: Path | None = None,
    attestation_bundle: Path | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    if gh_bin is None:
        gh_bin = fake_gh_path(material.parent.parent)
    if attestation_bundle is None:
        attestation_bundle = material.parent / INTEGRITY_ATTESTATION_BUNDLE_NAME
    cmd = [
        "python3",
        str(VERIFY_INTEGRITY),
        str(material),
        "--repo-root",
        str(REPO_ROOT),
        "--dist-dir",
        str(material.parent),
        "--release-tag",
        "v0.0.0",
        "--commit-sha",
        INTEGRITY_COMMIT_SHA,
        "--trust-policy",
        str(trust_policy_path(material.parent)),
        "--attestation-bundle",
        str(attestation_bundle),
        "--attestation-metadata",
        str(material.parent / "m80-release-attestation.json"),
        "--verification-time",
        INTEGRITY_VERIFICATION_TIME,
        "--gh-bin",
        str(gh_bin),
        "--rust-toolchain",
        INTEGRITY_RUST_TOOLCHAIN,
    ]
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def run_write_attestation_metadata(
    material: Path,
    metadata: Path,
    *,
    gh_bin: Path | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    if gh_bin is None:
        gh_bin = fake_gh_path(material.parent.parent)
    cmd = [
        "python3",
        str(WRITE_ATTESTATION_METADATA),
        "--material",
        str(material),
        "--attestation-bundle",
        str(material.parent / INTEGRITY_ATTESTATION_BUNDLE_NAME),
        "--trust-policy",
        str(trust_policy_path(material.parent)),
        "--release-tag",
        "v0.0.0",
        "--commit-sha",
        INTEGRITY_COMMIT_SHA,
        "--out",
        str(metadata),
        "--gh-bin",
        str(gh_bin),
    ]
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def rewrite_tar(
    src: Path,
    dst: Path,
    *,
    omit: set[str] | None = None,
    duplicate: str | None = None,
    metadata_updates: dict | None = None,
    metadata_file_updates: dict[str, str] | None = None,
    payload_updates: dict[str, object] | None = None,
    mode_updates: dict[str, int] | None = None,
    extra: dict[str, bytes] | None = None,
) -> None:
    omit = omit or set()
    mode_updates = mode_updates or {}
    payload_updates = payload_updates or {}
    extra = extra or {}
    entries: list[tuple[str, bytes, int]] = []
    with tarfile.open(src, "r:gz") as tar:
        for member in tar.getmembers():
            if not member.isfile() or member.name in omit:
                continue
            data = tar.extractfile(member).read()  # type: ignore[union-attr]
            if member.name == "bundle.json":
                metadata = json.loads(data.decode("utf-8"))
                if metadata_updates:
                    metadata.update(metadata_updates)
                if metadata_file_updates:
                    for row in metadata["files"]:
                        if row["path"] in metadata_file_updates:
                            row["sha256"] = metadata_file_updates[row["path"]]
                data = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode()
            if member.name in payload_updates:
                data = payload_updates[member.name](data)
            mode = mode_updates.get(member.name, member.mode & 0o777)
            entries.append((member.name, data, mode))
            if member.name == duplicate:
                entries.append((member.name, data, mode))
    for name, data in extra.items():
        entries.append((name, data, 0o644))
    with tarfile.open(dst, "w:gz") as tar:
        for name, data, mode in entries:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            tar.addfile(info, fileobj=BytesReader(data))


def file_mode(path: Path) -> int:
    return path.stat().st_mode & 0o777


def sha256(path: Path) -> str:
    import hashlib

    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_sha256_sidecar(path: Path, asset: Path, asset_name: str) -> None:
    path.write_text(f"{sha256(asset)}  {asset_name}\n")
    path.chmod(0o644)


def write_public_sha256s(path: Path, assets: list[tuple[str, Path]]) -> None:
    lines = [f"{sha256(asset)}  {name}\n" for name, asset in assets]
    path.write_text("".join(lines))
    path.chmod(0o644)


class BytesReader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        self.offset = 0

    def read(self, size: int = -1) -> bytes:
        if size == -1:
            size = len(self.data) - self.offset
        chunk = self.data[self.offset : self.offset + size]
        self.offset += len(chunk)
        return chunk


if __name__ == "__main__":
    unittest.main()
