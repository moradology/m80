#!/usr/bin/env python3
"""Unit tests for scripts/package-release-bundle.py."""

from __future__ import annotations

import json
from pathlib import Path
import runpy
import shutil
import shlex
import subprocess
import tarfile
import tempfile
import unittest

from quickstart_snippets import expected_quickstart_snippets, extract_marked_quickstart_snippets


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "package-release-bundle.py"
VERIFY = REPO_ROOT / "scripts" / "verify-release-bundle.py"
VERIFY_INTEGRITY = REPO_ROOT / "scripts" / "verify-release-integrity.py"
RELEASE_INTEGRITY_CONTRACT = REPO_ROOT / "docs" / "behaviors" / "release" / "release-integrity-contract.json"
UPLOAD_MANIFEST = REPO_ROOT / "scripts" / "release_upload_manifest.py"
PUBLISH_RECEIPT = REPO_ROOT / "scripts" / "release_publish_receipt.py"
REMOTE_INVENTORY = REPO_ROOT / "scripts" / "release_remote_asset_inventory.py"
EVIDENCE_BUNDLE = REPO_ROOT / "scripts" / "release_evidence_bundle.py"
WRITE_ATTESTATION_METADATA = REPO_ROOT / "scripts" / "write-release-attestation-metadata.py"
BUNDLE_NAME = "m80-linux-x86_64.tar.gz"
METADATA_NAME = "m80-linux-x86_64.bundle.json"
ASSET_INDEX_NAME = "m80-release-assets.json"
BOOTSTRAP_SELECTOR_NAME = "m80-bootstrap-selector.tsv"
BUILD_MANIFEST_NAME = "m80-release-build.json"
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
UPLOAD_MANIFEST_NAME = "m80-release-upload-manifest.json"
PUBLISH_RECEIPT_NAME = "m80-release-publish-decision.json"
REMOTE_INVENTORY_NAME = "m80-release-remote-assets.json"
EVIDENCE_BUNDLE_NAME = "m80-release-evidence.json"
HOSTLESS_QUICKSTART_PROOF_NAME = "m80-quickstart-proof-hostless.json"
REAL_KVM_QUICKSTART_PROOF_NAME = "m80-quickstart-proof-real-kvm.json"
RELEASE_PROOF_LEDGER_NAME = "m80-release-proof-ledger.jsonl"
VALID_CONTAINER_DIGEST = "sha256:" + ("a" * 64)
RELEASE_TARGET = "linux-x86_64"
RELEASE_TARGET_TRIPLE = "x86_64-unknown-linux-gnu"


class ReleaseBundleTest(unittest.TestCase):
    def test_packages_release_bundle_with_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
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
            self.assertTrue((out_dir / BUILD_MANIFEST_NAME).is_file())
            self.assertTrue((out_dir / f"{BUILD_MANIFEST_NAME}.sha256").is_file())
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
            self.assertEqual(file_mode(out_dir / BUILD_MANIFEST_NAME), 0o644)
            self.assertEqual(file_mode(out_dir / f"{BUILD_MANIFEST_NAME}.sha256"), 0o644)
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

            self.assertEqual(metadata["release_tag"], "v0.2.9")
            self.assertEqual(metadata["m80_version"], "v0.2.9")
            self.assertEqual(metadata["package_version"], "0.2.9")
            self.assertEqual(metadata["target"], "linux-x86_64")
            self.assertEqual(metadata["os"], "linux")
            self.assertEqual(metadata["arch"], "x86_64")
            self.assertEqual(metadata["image_kind"], "minimal")
            self.assertEqual(metadata["m80_protocol_version"], 1)
            self.assertEqual(metadata["guestd_package_version"], "0.2.9")
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
            self.assertIn("M80_RELEASE_TAG='v0.2.9'", public_install)
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
            self.assertIn('"$extract_dir/bin/m80" install --bundle-url "$selected_bundle_url"', public_install)
            self.assertNotIn('\nexec "$extract_dir/bin/m80"', public_install)
            self.assertNotIn("@M80_", public_install)
            self.assertNotIn("m80 quickstart", public_install)
            self.assertNotIn("quickstart.sh", public_install)
            index = json.loads((out_dir / ASSET_INDEX_NAME).read_text())
            self.assertEqual(index["schema_version"], 1)
            self.assertEqual(index["release_tag"], "v0.2.9")
            self.assertEqual(len(index["assets"]), 1)
            asset = index["assets"][0]
            self.assertEqual(asset["name"], BUNDLE_NAME)
            self.assertEqual(
                asset["url"],
                f"https://github.com/moradology/m80/releases/download/v0.2.9/{BUNDLE_NAME}",
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
            self.assertEqual(asset["m80_version"], "v0.2.9")
            self.assertEqual(asset["guest_protocol_version"], 1)
            self.assertEqual(asset["manifest_schema_version"], 5)
            self.assertEqual(asset["expected_firecracker_version"], "v1.15.1")
            selector = (out_dir / BOOTSTRAP_SELECTOR_NAME).read_text().splitlines()
            self.assertEqual(selector[0], "schema_version\t1")
            self.assertEqual(selector[1], "release_tag\tv0.2.9")
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
            self.assertEqual(selector_row[13], "v0.2.9")
            build_manifest = json.loads((out_dir / BUILD_MANIFEST_NAME).read_text())
            self.assertEqual(build_manifest["schema_version"], 1)
            self.assertEqual(build_manifest["release_tag"], "v0.2.9")
            self.assertEqual(build_manifest["source_commit"], INTEGRITY_COMMIT_SHA)
            self.assertEqual(build_manifest["rust_toolchain"], INTEGRITY_RUST_TOOLCHAIN)
            self.assertEqual(build_manifest["target"], "linux-x86_64")
            self.assertEqual(
                build_manifest["target_triples"],
                ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"],
            )
            self.assertEqual(build_manifest["m80_package_version"], "0.2.9")
            self.assertEqual(build_manifest["image_kind"], "minimal")
            self.assertEqual(build_manifest["cargo_lock_sha256"], sha256(REPO_ROOT / "Cargo.lock"))
            self.assertEqual(build_manifest["builder_identity"], "test-builder")
            self.assertEqual(build_manifest["builder_os_image"], "test-os-image")
            self.assertEqual(
                build_manifest["apt_packages"],
                [
                    {"name": "busybox-static", "version": "1.36.1"},
                    {"name": "curl", "version": "8.5.0"},
                    {"name": "e2fsprogs", "version": "1.47.0"},
                    {"name": "musl-tools", "version": "1.2.4"},
                ],
            )
            self.assertIsNone(build_manifest["container_digest"])
            self.assertEqual(build_manifest["bundle_metadata_name"], METADATA_NAME)
            self.assertEqual(build_manifest["bundle_metadata_sha256"], sha256(out_dir / METADATA_NAME))
            integrity = json.loads((out_dir / INTEGRITY_NAME).read_text())
            self.assertEqual(integrity["schema_version"], 1)
            self.assertEqual(integrity["mechanism"], "github-artifact-attestation")
            self.assertEqual(integrity["repository"], "moradology/m80")
            self.assertEqual(integrity["release_tag"], "v0.2.9")
            self.assertEqual(integrity["commit_sha"], INTEGRITY_COMMIT_SHA)
            self.assertEqual(integrity["target"], "linux-x86_64")
            self.assertEqual(integrity["rust_toolchain"], INTEGRITY_RUST_TOOLCHAIN)
            self.assertEqual(integrity["m80_package_version"], "0.2.9")
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
                    BUILD_MANIFEST_NAME,
                    f"{BUILD_MANIFEST_NAME}.sha256",
                    "SHA256SUMS",
                },
            )

    def test_package_accepts_container_only_builder_material(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            out_dir = root / "out"

            run_package(
                inputs,
                out_dir,
                apt_package_versions=[],
                container_digest=VALID_CONTAINER_DIGEST,
            )

            build_manifest = json.loads((out_dir / BUILD_MANIFEST_NAME).read_text())
            self.assertEqual(build_manifest["apt_packages"], [])
            self.assertEqual(build_manifest["container_digest"], VALID_CONTAINER_DIGEST)
            run_verify(out_dir / BUNDLE_NAME, verify_sidecars=True)
            run_verify_integrity(write_integrity_material(out_dir))

    def test_package_accepts_apt_and_container_builder_material(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            out_dir = root / "out"

            run_package(inputs, out_dir, container_digest=VALID_CONTAINER_DIGEST)

            build_manifest = json.loads((out_dir / BUILD_MANIFEST_NAME).read_text())
            self.assertTrue(build_manifest["apt_packages"])
            self.assertEqual(build_manifest["container_digest"], VALID_CONTAINER_DIGEST)

    def test_package_rejects_missing_builder_material(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")

            result = run_package(inputs, root / "out", apt_package_versions=[], check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release build manifest must record apt package versions or a container digest", result.stderr)

    def test_package_rejects_malformed_container_digest_values(self) -> None:
        invalid_values = [
            "latest",
            "example.com/m80-builder:latest",
            "sha256:" + ("0" * 63),
            "sha256:" + ("A" * 64),
            "sha512:" + ("0" * 128),
            "sha256:" + ("0" * 64) + " tag",
            "sha256:" + ("0" * 64) + "\n",
        ]
        for invalid_digest in invalid_values:
            with self.subTest(invalid_digest=invalid_digest):
                with tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    inputs = fixture_inputs(root, release_tag="v0.2.9")

                    result = run_package(
                        inputs,
                        root / "out",
                        container_digest=invalid_digest,
                        check=False,
                    )

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("container digest must be sha256:<64 lowercase hex>", result.stderr)

    def test_package_rejects_legacy_quickstart_install_script(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            legacy = root / "legacy-quickstart.sh"
            write_executable(
                legacy,
                "#!/bin/sh\nexec \"${M80_BIN:-m80}\" quickstart --artifact-url \"$1\"\n",
            )
            inputs["install"] = legacy

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must use selector-driven m80 install", result.stderr)

    def test_package_rejects_literal_quickstart_script_template(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            legacy = root / "literal-quickstart.sh"
            write_executable(
                legacy,
                "#!/bin/sh\n"
                "M80_RELEASE_TAG='@M80_RELEASE_TAG@'\n"
                "# do not delegate back to scripts/quickstart.sh\n"
                "bin/m80 install --bundle-url file://bundle.tgz\n",
            )
            inputs["install"] = legacy

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must use selector-driven m80 install", result.stderr)

    def test_package_rejects_interactive_install_prompt(self) -> None:
        cases = {
            "line-start": "read answer\n",
            "inline": "if read -r answer; then exit 1; fi\n",
            "select": "select answer in yes no; do exit 1; done\n",
        }
        for name, prompt in cases.items():
            with self.subTest(name=name):
                with tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    inputs = fixture_inputs(root, release_tag="v0.2.9")
                    interactive = root / "interactive-install.sh"
                    write_executable(
                        interactive,
                        "#!/bin/sh\n"
                        "M80_RELEASE_TAG='@M80_RELEASE_TAG@'\n"
                        "M80_PUBLIC_RELEASE_OWNER='@M80_PUBLIC_RELEASE_OWNER@'\n"
                        "M80_PUBLIC_RELEASE_REPO='@M80_PUBLIC_RELEASE_REPO@'\n"
                        "M80_BOOTSTRAP_SELECTOR_NAME='m80-bootstrap-selector.tsv'\n"
                        "M80_ASSET_INDEX_NAME='m80-release-assets.json'\n"
                        f"{prompt}"
                        "\"bin/m80\" install --bundle-url \"file://bundle.tgz\"\n",
                    )
                    inputs["install"] = interactive

                    result = run_package(inputs, root / "out", check=False)

                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("install.sh template must be noninteractive", result.stderr)

    def test_package_rejects_unsupported_install_template_placeholders(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            placeholder = root / "placeholder-install.sh"
            write_executable(
                placeholder,
                "#!/bin/sh\n"
                "M80_RELEASE_TAG='@M80_RELEASE_TAG@'\n"
                "M80_PUBLIC_RELEASE_OWNER='@M80_PUBLIC_RELEASE_OWNER@'\n"
                "M80_PUBLIC_RELEASE_REPO='@M80_PUBLIC_RELEASE_REPO@'\n"
                "M80_NEW_UNRENDERED='@M80_NEW_UNRENDERED@'\n"
                "M80_MIXED_UNRENDERED='@M80_mixed_Unrendered@'\n"
                "M80_BOOTSTRAP_SELECTOR_NAME='m80-bootstrap-selector.tsv'\n"
                "M80_ASSET_INDEX_NAME='m80-release-assets.json'\n"
                "\"bin/m80\" install --bundle-url \"file://bundle.tgz\"\n",
            )
            inputs["install"] = placeholder

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("@M80_mixed_Unrendered@", result.stderr)
            self.assertIn("unsupported placeholder(s): @M80_NEW_UNRENDERED@", result.stderr)

    def test_package_rejects_hardcoded_public_bundle_install_script(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            hardcoded = root / "hardcoded-install.sh"
            write_executable(
                hardcoded,
                "#!/bin/sh\n"
                "M80_RELEASE_TAG='@M80_RELEASE_TAG@'\n"
                "curl -fsSL https://github.com/moradology/m80/releases/download/v0.2.9/m80-linux-x86_64.tar.gz -o bundle.tgz\n"
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
            base = "https://github.com/moradology/m80/releases/download/v0.2.9"
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
                    f"{base}/{BUILD_MANIFEST_NAME}",
                    f"{base}/{BUILD_MANIFEST_NAME}.sha256",
                    f"{base}/SHA256SUMS",
                    f"{base}/{BUNDLE_NAME}",
                    f"{base}/{BUNDLE_NAME}.sha256",
                ],
            )
            self.assertIn("verified release tag=v0.2.9", result.stderr)
            self.assertIn(
                f"verified assets={BUNDLE_NAME},{METADATA_NAME},{INSTALL_NAME},{BUILD_MANIFEST_NAME},{INTEGRITY_NAME},{INTEGRITY_ATTESTATION_BUNDLE_NAME}",
                result.stderr,
            )
            self.assertIn(f"install_sh_sha256={sha256(root / 'out' / INSTALL_NAME)}", result.stderr)
            self.assertIn(
                "verified handoff binary=v0.2.9 "
                f"source_commit={INTEGRITY_COMMIT_SHA} "
                f"target={RELEASE_TARGET} target_triple={RELEASE_TARGET_TRIPLE} "
                "protocol=1 manifest_schema=5",
                result.stderr,
            )
            self.assertTrue(install_args.is_file(), result.stderr)
            args = install_args.read_text().splitlines()
            self.assertEqual(args[0:2], ["install", "--bundle-url"])
            self.assertEqual(args[2], f"{base}/{BUNDLE_NAME}")
            self.assertEqual(args[3:], ["--dry-run"])

    def test_package_assembles_multi_tuple_release_from_tuple_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            seed_out = root / "seed"
            out_dir = root / "out"
            run_package(inputs, seed_out)
            manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
            run_package(inputs, out_dir, extra_tuple_manifests=[manifest])
            material = out_dir / INTEGRITY_NAME
            write_trust_policy(out_dir)
            write_attestation_bundle(out_dir, material)
            write_attestation_metadata(out_dir, material)
            write_fake_gh(root)

            run_verify(out_dir / BUNDLE_NAME, verify_sidecars=True)
            run_verify_integrity(material)
            result, urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("m80-linux-x86_64-debug.tar.gz", "\n".join(urls))
            self.assertNotIn("m80-linux-x86_64-debug.bundle.json", "\n".join(urls))
            index = json.loads((out_dir / ASSET_INDEX_NAME).read_text())
            self.assertEqual(
                [(asset["os"], asset["arch"], asset["image_kind"]) for asset in index["assets"]],
                [("linux", "x86_64", "debug"), ("linux", "x86_64", "minimal")],
            )
            debug_asset = index["assets"][0]
            self.assertEqual(debug_asset["name"], "m80-linux-x86_64-debug.tar.gz")
            self.assertEqual(debug_asset["metadata_name"], "m80-linux-x86_64-debug.bundle.json")
            for asset_name in [
                "m80-linux-x86_64-debug.tar.gz",
                "m80-linux-x86_64-debug.tar.gz.sha256",
                "m80-linux-x86_64-debug.bundle.json",
                "m80-linux-x86_64-debug.bundle.json.sha256",
            ]:
                self.assertTrue((out_dir / asset_name).is_file(), asset_name)
                self.assertIn(f"  {asset_name}\n", (out_dir / "SHA256SUMS").read_text())
            integrity = json.loads(material.read_text())
            subject_names = {subject["name"] for subject in integrity["subjects"]}
            self.assertIn("m80-linux-x86_64-debug.tar.gz", subject_names)
            self.assertIn("m80-linux-x86_64-debug.tar.gz.sha256", subject_names)
            self.assertIn("m80-linux-x86_64-debug.bundle.json", subject_names)
            self.assertIn("m80-linux-x86_64-debug.bundle.json.sha256", subject_names)
            install_argv = install_args.read_text().splitlines()
            self.assertEqual(install_argv[0:2], ["install", "--bundle-url"])
            self.assertTrue(install_argv[2].endswith(f"/{BUNDLE_NAME}"), install_argv)
            expected = expected_quickstart_snippets()
            for doc in [REPO_ROOT / "README.md", REPO_ROOT / "docs" / "runbook" / "release.md"]:
                snippets = extract_marked_quickstart_snippets(doc)
                self.assertEqual(snippets["latest-install"], expected["latest-install"])
                self.assertEqual(snippets["pinned-install"], expected["pinned-install"])

    def test_package_rejects_extra_tuple_name_collision_before_copy(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            seed_out = root / "seed"
            out_dir = root / "out"
            run_package(inputs, seed_out)
            manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
            payload = json.loads(manifest.read_text())
            payload["bundle_name"] = BUNDLE_NAME
            manifest.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_package(inputs, out_dir, extra_tuple_manifests=[manifest], check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release tuple dist asset name collides", result.stderr)
            self.assertEqual(sha256(out_dir / BUNDLE_NAME), sha256(seed_out / BUNDLE_NAME))

    def test_package_rejects_extra_tuple_metadata_sidecar_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            seed_out = root / "seed"
            run_package(inputs, seed_out)
            manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
            payload = json.loads(manifest.read_text())
            metadata_path = Path(payload["metadata_path"])
            metadata = json.loads(metadata_path.read_text())
            metadata["image_kind"] = "stale-debug"
            metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")

            result = run_package(inputs, root / "out", extra_tuple_manifests=[manifest], check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("tuple bundle metadata sidecar mismatch", result.stderr)

    def test_package_rejects_extra_tuple_tar_internal_corruption(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            seed_out = root / "seed"
            out_dir = root / "out"
            run_package(inputs, seed_out)
            manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
            payload = json.loads(manifest.read_text())
            rewrite_tar(
                Path(payload["bundle_path"]),
                Path(payload["bundle_path"]).with_suffix(".broken.tar.gz"),
                payload_updates={"artifacts/m80-guestd": lambda _data: b"tampered guestd\n"},
            )
            Path(payload["bundle_path"]).with_suffix(".broken.tar.gz").replace(payload["bundle_path"])

            result = run_package(inputs, out_dir, extra_tuple_manifests=[manifest], check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("tuple bundle contract verification failed for m80-linux-x86_64-debug.tar.gz", result.stderr)
            self.assertIn("guest manifest daemon_binary_sha256 mismatch", result.stderr)

    def test_rendered_install_script_rejects_extracted_m80_source_commit_mismatch_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                fake_m80_script(
                    fake_m80_payload(updates={"source_commit": "1" * 40})
                ),
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 source_commit mismatch", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_extracted_m80_release_tag_mismatch_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                fake_m80_script(fake_m80_payload(updates={"release_tag": "v9.9.9"})),
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 release_tag mismatch", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_extracted_m80_target_mismatch_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                fake_m80_script(fake_m80_payload(updates={"target": "linux-aarch64"})),
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 target mismatch", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_extracted_m80_target_triple_mismatch_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                fake_m80_script(fake_m80_payload(updates={"target_triple": "aarch64-unknown-linux-gnu"})),
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 target_triple missing from build manifest target_triples", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_extracted_m80_dev_identity_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                fake_m80_script(
                    fake_m80_payload(
                        updates={
                            "binary_version": "0.2.9-dev",
                            "release_tag": None,
                            "release_build": False,
                            "version_status": "dev",
                            "source_commit": None,
                        }
                    )
                ),
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 version_status mismatch", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_extracted_m80_malformed_identity_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                "#!/bin/sh\n"
                "if [ \"${1:-}\" = install ]; then exit 99; fi\n"
                "printf 'not-json\\n'\n",
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 identity verification failed", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_rejects_extracted_m80_missing_identity_field_before_install(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)
            replace_bundle_m80_for_install(
                root / "out",
                fake_m80_script(fake_m80_payload(omit={"protocol_version"})),
            )

            result, _urls, install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("extracted m80 identity missing field(s): protocol_version", result.stderr)
            self.assertFalse(install_args.exists())

    def test_rendered_install_script_bounds_every_download(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_signed_fixture(root)

            result, urls, _install_args = run_rendered_install(root, args=["--dry-run"])

            self.assertEqual(result.returncode, 0, result.stderr)
            arg_lines = rendered_install_curl_arg_lines(root)
            self.assertEqual(len(arg_lines), len(urls))
            for line in arg_lines:
                args = shlex.split(line)
                self.assert_curl_flag(args, "--connect-timeout", "10")
                self.assert_curl_flag(args, "--max-time", "120")
                self.assert_curl_flag(args, "--retry", "2")
                self.assert_curl_flag(args, "--retry-delay", "1")

    def test_rendered_install_script_download_failures_are_diagnosable_before_extract(self) -> None:
        cases = [
            (28, "timeout"),
            (6, "dns_or_connect_failure"),
            (22, "http_failure"),
            (130, "interrupted"),
        ]
        for exit_code, failure in cases:
            with self.subTest(exit_code=exit_code, failure=failure):
                with tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    package_signed_fixture(root)
                    curl_script = (
                        "#!/bin/sh\n"
                        "printf 'simulated curl failure\\n' >&2\n"
                        f"exit {exit_code}\n"
                    )

                    result, _urls, install_args = run_rendered_install(root, curl_script=curl_script)

                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("release_tag=v0.2.9", result.stderr)
                    self.assertIn(f"asset={BOOTSTRAP_SELECTOR_NAME}", result.stderr)
                    self.assertIn(failure, result.stderr)
                    self.assertIn("verification_started=no", result.stderr)
                    self.assertIn(
                        f"url=https://github.com/moradology/m80/releases/download/v0.2.9/{BOOTSTRAP_SELECTOR_NAME}",
                        result.stderr,
                    )
                    self.assertFalse((root / "tar.log").exists(), result.stderr)
                    self.assertFalse(install_args.exists(), result.stderr)

    def assert_curl_flag(self, args: list[str], flag: str, expected_value: str) -> None:
        self.assertIn(flag, args)
        pos = args.index(flag)
        self.assertLess(pos + 1, len(args), args)
        self.assertEqual(args[pos + 1], expected_value, args)

    def test_rendered_install_script_rejects_unsigned_dev_fixture_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"asset={INTEGRITY_ATTESTATION_BUNDLE_NAME}", result.stderr)
            self.assertIn("verification_started=yes", result.stderr)
            self.assertIn("failure=http_failure", result.stderr)
            self.assertIn(
                "retry pinned command: curl -fsSL --connect-timeout 10 --max-time 120 --retry 2 --retry-delay 1 https://github.com/moradology/m80/releases/download/v0.2.9/install.sh | sudo sh",
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

    def test_rendered_install_script_rejects_build_manifest_commit_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"source_commit": "1" * 40})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest source_commit mismatch", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_metadata_hash_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"bundle_metadata_sha256": "0" * 64})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest bundle_metadata_sha256 mismatch", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_without_builder_material_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"apt_packages": [], "container_digest": None})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest missing apt packages or container digest", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_malformed_container_digest_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"container_digest": "m80-builder:latest"})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest container_digest invalid", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_target_triples_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(
                root / "out",
                {
                    "target_triples": ["x86_64-unknown-linux-gnu"],
                },
            )
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest target_triples missing x86_64-unknown-linux-musl", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_rust_toolchain_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"rust_toolchain": "0.0"})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest rust_toolchain mismatch", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_target_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"target": "linux-aarch64"})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest target mismatch: expected linux-x86_64", result.stderr)
            self.assertIn("retry pinned command:", result.stderr)
            assert_no_bundle_download(self, urls, install_args)
            self.assertFalse((root / "tar.log").exists())
            self.assertFalse(install_root.exists())

    def test_rendered_install_script_rejects_build_manifest_package_version_mismatch_before_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"m80_package_version": "9.9.9"})
            write_integrity_material(root / "out")
            install_root = root / "install-root"

            result, urls, install_args = run_rendered_install(
                root,
                args=["--install-root", str(install_root)],
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest m80_package_version mismatch", result.stderr)
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
            add_alternate_image_kind_fixture(root / "out")
            lines = bootstrap_selector_lines(root / "out")
            rewrite_bootstrap_selector(
                root / "out",
                lines[:3] + bootstrap_selector_rows_for_image_kind(lines, "debug"),
            )

            result, urls, install_args = run_rendered_install(root)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("image_kind=minimal", result.stderr)
            self.assertIn("bootstrap selector missing tuple", result.stderr)
            self.assertIn("available_tuples=linux/x86_64/debug", result.stderr)
            self.assertNotIn("curl -fsSL", result.stderr)
            self.assertNotIn("quickstart", result.stderr)
            self.assertNotIn("README", result.stderr)
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

    def test_install_script_is_posix_shellclean(self) -> None:
        subprocess.run(["sh", "-n", str(REPO_ROOT / "scripts" / "install.sh")], check=True)
        if shutil.which("shellcheck") is None:
            self.skipTest("shellcheck is not installed")
        subprocess.run(["shellcheck", "-s", "sh", str(REPO_ROOT / "scripts" / "install.sh")], check=True)

    def test_ci_runs_release_script_tests(self) -> None:
        workflow = (REPO_ROOT / ".github/workflows/ci.yml").read_text()

        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/package-release-bundle.py")
        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/release_publish_authority.py")
        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/release_evidence_bundle.py")
        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/release_proof_ledger.py")
        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/freshness_failure_policy.py")
        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/current_latest_repair_preflight.py")
        self.assertRegex(workflow, r"python3 -m py_compile .*scripts/test-workflow-policy.py")
        self.assertIn("python3 scripts/test-release-url-contract.py", workflow)
        self.assertIn("python3 scripts/test-freshness-failure-policy.py", workflow)
        self.assertIn("python3 scripts/verify-freshness-failure-policy.py", workflow)
        self.assertIn("python3 scripts/test-current-latest-repair-preflight.py", workflow)
        self.assertIn("python3 scripts/test-workflow-policy.py", workflow)
        self.assertIn("python3 scripts/test-release-proof-ledger.py", workflow)
        self.assertIn(
            "python3 scripts/verify-release-tracker-policy.py --policy-config "
            "docs/behaviors/release/release-tracker-policy.json",
            workflow,
        )
        self.assertNotIn("\n          python3 scripts/verify-release-tracker-policy.py\n", workflow)
        self.assertIn("python3 scripts/test-release-bundle.py", workflow)
        self.assertIn('FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: "true"', workflow)
        self.assertIn("uses: actions/checkout@v6", workflow)
        self.assertIn("uses: actions/cache@v5", workflow)
        self.assertNotIn("actions/checkout@v4", workflow)
        self.assertNotIn("actions/cache@v4", workflow)
        self.assertIn("sudo apt-get install -y erofs-utils shellcheck", workflow)
        self.assertIn("sh -n scripts/install.sh", workflow)
        self.assertIn("shellcheck -s sh scripts/install.sh", workflow)
        self.assertIn("! -name 'install.sh'", workflow)
        self.assertIn("-print0 | sort -z | xargs -0 shellcheck -s bash", workflow)
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
        self.assertIn("fetch-depth: 0", workflow)
        self.assertNotIn("actions/checkout@v4", workflow)
        self.assertIn("actions/attest@v4", workflow)
        self.assertIn("cargo build --locked -p m80-image-build --release", workflow)
        self.assertIn("cargo build --locked -p m80-guestd --release --target x86_64-unknown-linux-musl", workflow)
        self.assertIn("cargo build --locked -p m80-cli -p m80-jailer-harden -p m80-net-helper --release", workflow)
        self.assertIn("release_commit=\"$(git rev-parse HEAD)\"", workflow)
        self.assertIn("release_target_triple=\"$(rustc -vV | awk '/^host:/ {print $2}')\"", workflow)
        self.assertIn("M80_RELEASE_COMMIT=\"$release_commit\"", workflow)
        self.assertIn("M80_RELEASE_TARGET_TRIPLE=\"$release_target_triple\"", workflow)
        self.assertIn("--target-triple x86_64-unknown-linux-musl", workflow)
        self.assertIn("--builder-identity", workflow)
        self.assertIn("--builder-os-image", workflow)
        self.assertIn("--apt-package-version", workflow)
        self.assertIn("subject-name: m80-release-integrity.json", workflow)
        self.assertIn("subject-digest: ${{ steps.integrity-subject.outputs.digest }}", workflow)
        self.assertNotIn("subject-path:", workflow)
        self.assertIn("release_commit: ${{ steps.release-commit.outputs.sha }}", workflow)
        self.assertIn("release_commit_sha=\"$(git rev-parse HEAD)\"", workflow)
        self.assertIn('echo "sha=$release_commit_sha" >> "$GITHUB_OUTPUT"', workflow)
        self.assertNotIn('echo "sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"', workflow)
        self.assertIn("Capture current latest release metadata", workflow)
        self.assertIn("Preflight current latest repair target", workflow)
        self.assertIn("scripts/current_latest_repair_preflight.py", workflow)
        self.assertIn("--latest-metadata \"$ARTIFACT_DIR/current-latest-release.json\"", workflow)
        self.assertIn("--out \"$ARTIFACT_DIR/m80-current-latest-repair-preflight.json\"", workflow)
        self.assertIn("m80-current-latest-repair-preflight-${{ github.run_id }}", workflow)
        self.assertLess(
            workflow.index("scripts/current_latest_repair_preflight.py"),
            workflow.index("scripts/package-release-bundle.py"),
        )
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
        self.assertIn("scripts/release_proof_ledger.py append", workflow)
        self.assertIn('--result-out "$DIST_DIR/m80-quickstart-proof-hostless.verifier-result.json"', workflow)
        self.assertIn('--verifier-result "$DIST_DIR/m80-quickstart-proof-hostless.verifier-result.json"', workflow)
        self.assertIn('--log-artifact "$DIST_DIR/m80-quickstart-stderr.txt"', workflow)
        self.assertIn("--runner-identity", workflow)
        self.assertGreaterEqual(workflow.count("scripts/release_proof_ledger.py verify"), 2)
        self.assertGreaterEqual(workflow.count("--expect-record-count 1"), 2)
        self.assertIn("m80-quickstart-proof-hostless.json", workflow)
        self.assertIn("m80-quickstart-proof-hostless.verifier-result.json", workflow)
        self.assertIn("m80-release-proof-ledger.jsonl", workflow)
        self.assertGreaterEqual(workflow.count("scripts/release_upload_manifest.py"), 5)
        self.assertIn("Write and validate release upload manifest", workflow)
        self.assertIn("--write", workflow)
        self.assertIn("Verify release upload manifest before upload", workflow)
        self.assertIn("Verify publish authority before mutation", workflow)
        self.assertIn("scripts/release_publish_authority.py", workflow)
        self.assertIn("M80_RELEASE_TOKEN_SOURCE: github.token", workflow)
        self.assertLess(
            workflow.index("scripts/release_publish_authority.py"),
            workflow.index("scripts/release_publish_receipt.py"),
        )
        self.assertIn("Write and validate publish decision receipt before upload", workflow)
        self.assertIn("scripts/release_publish_receipt.py", workflow)
        self.assertIn("--workflow-run-id \"$GITHUB_RUN_ID\"", workflow)
        self.assertIn("--actor \"$GITHUB_ACTOR\"", workflow)
        self.assertLess(
            workflow.index("scripts/release_publish_receipt.py"),
            workflow.index("scripts/release_publication_plan.py"),
        )
        self.assertIn("Resolve release publication plan", workflow)
        self.assertIn("m80-release-publication-plan.json", workflow)
        self.assertIn("create_draft_upload_publish)", workflow)
        self.assertIn('gh release create "$GITHUB_REF_NAME"', workflow)
        self.assertIn("--verify-tag", workflow)
        self.assertIn("--draft", workflow)
        self.assertIn('gh release edit "$GITHUB_REF_NAME" --draft=false --latest=false --verify-tag', workflow)
        self.assertIn('gh release edit "$GITHUB_REF_NAME" --latest --verify-tag', workflow)
        self.assertIn("validate_existing_public_release)", workflow)
        self.assertIn("mapfile -t upload_paths < <(", workflow)
        self.assertIn("--print-upload-paths", workflow)
        self.assertIn('gh release upload "$GITHUB_REF_NAME" "${upload_paths[@]}"', workflow)
        self.assertNotIn("--clobber", workflow)
        self.assertIn("Validate uploaded draft before latest promotion", workflow)
        self.assertIn("download_args=(--dir /tmp/m80-release-prepublish)", workflow)
        self.assertIn("/tmp/m80-release-prepublish/m80-linux-x86_64.tar.gz", workflow)
        self.assertIn("Publish validated draft without latest promotion", workflow)
        self.assertIn("Mark validated release as latest", workflow)
        self.assertIn("python3 scripts/stable_release_channel.py", workflow)
        self.assertIn("Write no-auth public-access release readiness receipt", workflow)
        self.assertIn("scripts/release_public_access_receipt.py", workflow)
        self.assertIn("GH_CONFIG_DIR=/tmp/m80-noauth-gh env -u GH_TOKEN -u GITHUB_TOKEN", workflow)
        self.assertIn("release-readiness-public-access.json", workflow)
        self.assertLess(
            workflow.index('gh release upload "$GITHUB_REF_NAME" "${upload_paths[@]}"'),
            workflow.index("Validate uploaded draft before latest promotion"),
        )
        self.assertLess(
            workflow.index("Validate uploaded draft before latest promotion"),
            workflow.index("Publish validated draft without latest promotion"),
        )
        self.assertLess(
            workflow.index("Publish validated draft without latest promotion"),
            workflow.index("Re-download and validate published release assets"),
        )
        self.assertLess(
            workflow.index("Re-download and validate published release assets"),
            workflow.index("Mark validated release as latest"),
        )
        self.assertLess(
            workflow.index("Mark validated release as latest"),
            workflow.index("Write no-auth public-access release readiness receipt"),
        )
        self.assertLess(
            workflow.index("Write no-auth public-access release readiness receipt"),
            workflow.index("Upload public-access release readiness receipt"),
        )
        self.assertIn("download_args=(--dir /tmp/m80-release-redownload)", workflow)
        self.assertIn('download_args+=(--pattern "$pattern")', workflow)
        self.assertIn("--print-download-patterns", workflow)
        self.assertIn('gh release download "$GITHUB_REF_NAME" "${download_args[@]}"', workflow)
        self.assertIn("--manifest /tmp/m80-release-upload/m80-release-upload-manifest.json", workflow)
        self.assertIn("--require-exact-dist-public-assets", workflow)
        self.assertLess(
            workflow.index("--require-exact-dist-public-assets"),
            workflow.index('gh api "repos/${GITHUB_REPOSITORY}/releases/tags/${GITHUB_REF_NAME}"'),
        )
        self.assertIn("scripts/release_remote_asset_inventory.py", workflow)
        self.assertIn("--redownload-dir /tmp/m80-release-redownload", workflow)
        self.assertIn("--release-metadata /tmp/m80-release-redownload/github-release.json", workflow)
        self.assertIn("github-release-assets.json", workflow)
        self.assertIn("gh api --paginate --slurp", workflow)
        self.assertIn("--release-assets-metadata /tmp/m80-release-redownload/github-release-assets.json", workflow)
        self.assertLess(
            workflow.index('gh api "repos/${GITHUB_REPOSITORY}/releases/tags/${GITHUB_REF_NAME}"'),
            workflow.index("gh api --paginate --slurp"),
        )
        self.assertLess(
            workflow.index("gh api --paginate --slurp"),
            workflow.index("scripts/release_remote_asset_inventory.py"),
        )
        self.assertLess(
            workflow.index("scripts/release_remote_asset_inventory.py"),
            workflow.rindex("scripts/verify-install-handoff.py"),
        )
        self.assertNotIn(
            'gh release upload "$GITHUB_REF_NAME" \\\n            /tmp/m80-release-upload/',
            workflow,
        )
        self.assertIn("actions/upload-artifact", workflow)
        self.assertIn("actions/download-artifact", workflow)
        self.assertIn("m80-release-publish-decision-${{ github.run_id }}", workflow)
        self.assertIn("/tmp/m80-release-upload/m80-release-publish-decision.json", workflow)
        self.assertIn("m80-release-publication-plan-${{ github.run_id }}", workflow)
        self.assertIn("/tmp/m80-release-upload/m80-release-publication-plan.json", workflow)
        self.assertIn("m80-release-remote-assets-${{ github.run_id }}", workflow)
        self.assertIn("/tmp/m80-release-redownload/m80-release-remote-assets.json", workflow)
        self.assertIn("m80-release-public-access-${{ github.run_id }}", workflow)
        self.assertIn("/tmp/m80-release-redownload/release-readiness-public-access.json", workflow)
        self.assertIn("/tmp/m80-release-prepublish/**", workflow)
        for name in [
            BUNDLE_NAME,
            f"{BUNDLE_NAME}.sha256",
            METADATA_NAME,
            f"{METADATA_NAME}.sha256",
            ASSET_INDEX_NAME,
            f"{ASSET_INDEX_NAME}.sha256",
            BOOTSTRAP_SELECTOR_NAME,
            f"{BOOTSTRAP_SELECTOR_NAME}.sha256",
            BUILD_MANIFEST_NAME,
            f"{BUILD_MANIFEST_NAME}.sha256",
            INSTALL_NAME,
            f"{INSTALL_NAME}.sha256",
            "SHA256SUMS",
            INTEGRITY_NAME,
            INTEGRITY_ATTESTATION_BUNDLE_NAME,
            INTEGRITY_ATTESTATION_METADATA_NAME,
        ]:
            self.assertNotIn(f"--pattern {name}", workflow)
        for name in [
            INTEGRITY_NAME,
            INTEGRITY_ATTESTATION_BUNDLE_NAME,
            INTEGRITY_ATTESTATION_METADATA_NAME,
        ]:
            self.assertGreaterEqual(
                workflow.count(f"/tmp/m80-release-redownload/{name}"),
                1,
                f"{name} must be re-verified after publication",
            )

    def test_release_upload_manifest_schema_derives_public_and_non_public_assets(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))

            manifest = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())
            material = json.loads((out_dir / INTEGRITY_NAME).read_text())
            public_assets = {asset["name"]: asset for asset in manifest["public_assets"]}
            integrity_subject_names = {subject["name"] for subject in material["subjects"]}
            non_public_names = {
                artifact["name"] for artifact in manifest["non_public_workflow_artifacts"]
            }

            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["release_tag"], "v0.2.9")
            self.assertEqual(
                {name for name, asset in public_assets.items() if asset["integrity_subject"]},
                integrity_subject_names,
            )
            self.assertIn(INTEGRITY_NAME, public_assets)
            self.assertEqual(public_assets[INTEGRITY_NAME]["kind"], "release-integrity-predicate")
            self.assertFalse(public_assets[INTEGRITY_NAME]["integrity_subject"])
            self.assertEqual(
                {
                    INTEGRITY_ATTESTATION_BUNDLE_NAME,
                    INTEGRITY_ATTESTATION_METADATA_NAME,
                },
                {
                    name
                    for name, asset in public_assets.items()
                    if asset["kind"]
                    in {
                        "github-artifact-attestation-bundle",
                        "release-attestation-metadata",
                    }
                },
            )
            self.assertIn(UPLOAD_MANIFEST_NAME, non_public_names)
            self.assertIn(HOSTLESS_QUICKSTART_PROOF_NAME, non_public_names)
            self.assertIn(RELEASE_PROOF_LEDGER_NAME, non_public_names)
            self.assertNotIn(UPLOAD_MANIFEST_NAME, public_assets)
            self.assertNotIn(HOSTLESS_QUICKSTART_PROOF_NAME, public_assets)
            self.assertNotIn(RELEASE_PROOF_LEDGER_NAME, public_assets)
            for name, asset in public_assets.items():
                self.assertEqual(asset["sha256"], sha256(out_dir / name))
                self.assertEqual(asset["size_bytes"], (out_dir / name).stat().st_size)

    def test_release_upload_manifest_prints_upload_paths_and_download_patterns(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())
            public_names = [asset["name"] for asset in manifest["public_assets"]]

            upload = run_release_upload_manifest(out_dir, "--print-upload-paths")
            download = run_release_upload_manifest(out_dir, "--print-download-patterns")

            self.assertEqual(upload.stdout.splitlines(), [str(out_dir / name) for name in public_names])
            self.assertEqual(download.stdout.splitlines(), public_names)

    def test_release_upload_manifest_rejects_extra_redownloaded_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            (redownload / "extra.txt").write_text("extra\n")

            result = run_release_upload_manifest(
                redownload,
                "--manifest",
                str(out_dir / UPLOAD_MANIFEST_NAME),
                "--require-exact-dist-public-assets",
                check=False,
            )

            self.assertIn("redownload file set mismatch", result.stderr)
            self.assertIn("extra.txt", result.stderr)

    def test_release_upload_manifest_rejects_missing_release_integrity_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            material_path = out_dir / INTEGRITY_NAME
            material = json.loads(material_path.read_text())
            removed = material["subjects"][0]["name"]
            material["subjects"] = material["subjects"][1:]
            material_path.write_text(json.dumps(material, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("public asset set mismatch", result.stderr)
            self.assertIn(removed, result.stderr)

    def test_release_upload_manifest_rejects_missing_sha256sum_coverage(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            sums_path = out_dir / "SHA256SUMS"
            lines = [
                line
                for line in sums_path.read_text().splitlines()
                if not line.endswith(f"  {INSTALL_NAME}")
            ]
            sums_path.write_text("\n".join(lines) + "\n")
            refresh_integrity_subject_and_manifest_asset(out_dir, "SHA256SUMS")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("SHA256SUMS subject coverage mismatch", result.stderr)
            self.assertIn(INSTALL_NAME, result.stderr)

    def test_release_upload_manifest_rejects_stale_provenance_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            material_path = out_dir / INTEGRITY_NAME
            material = json.loads(material_path.read_text())
            material["subjects"][0]["sha256"] = "0" * 64
            stale_name = material["subjects"][0]["name"]
            material_path.write_text(json.dumps(material, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn(f"release upload integrity subject {stale_name} sha256 mismatch", result.stderr)

    def test_release_upload_manifest_rejects_synthetic_public_asset_omitted_from_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            synthetic = out_dir / "future-installer.bin"
            synthetic.write_text("future\n")
            material_path = out_dir / INTEGRITY_NAME
            material = json.loads(material_path.read_text())
            material["subjects"].append(
                {
                    "kind": "installer",
                    "name": synthetic.name,
                    "sha256": sha256(synthetic),
                    "size_bytes": synthetic.stat().st_size,
                }
            )
            material_path.write_text(json.dumps(material, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("public asset set mismatch", result.stderr)
            self.assertIn(synthetic.name, result.stderr)

    def test_release_upload_manifest_rejects_duplicate_public_asset_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["public_assets"].append(dict(payload["public_assets"][0]))
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("duplicate name", result.stderr)

    def test_release_upload_manifest_rejects_path_traversal_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["public_assets"][0]["name"] = "../evil"
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("flat dist asset name", result.stderr)

    def test_release_upload_manifest_rejects_missing_public_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            (out_dir / INTEGRITY_ATTESTATION_BUNDLE_NAME).unlink()

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn(f"release upload public asset missing: {INTEGRITY_ATTESTATION_BUNDLE_NAME}", result.stderr)

    def test_release_upload_manifest_rejects_stale_public_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["public_assets"][0]["sha256"] = "0" * 64
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("sha256 mismatch", result.stderr)

    def test_release_upload_manifest_rejects_stale_public_size(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["public_assets"][0]["size_bytes"] += 1
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("size_bytes mismatch", result.stderr)

    def test_release_upload_manifest_rejects_unknown_top_level_field(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["unexpected"] = True
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("field mismatch", result.stderr)
            self.assertIn("unexpected", result.stderr)

    def test_release_upload_manifest_rejects_undocumented_public_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            extra = out_dir / "extra.txt"
            extra.write_text("extra\n")
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["public_assets"].append(
                {
                    "name": extra.name,
                    "kind": "debug-extra",
                    "sha256": sha256(extra),
                    "size_bytes": extra.stat().st_size,
                    "integrity_subject": False,
                }
            )
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("public asset set mismatch", result.stderr)
            self.assertIn("extra.txt", result.stderr)

    def test_release_upload_manifest_rejects_undocumented_non_public_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            payload = json.loads(manifest_path.read_text())
            payload["non_public_workflow_artifacts"].append(
                {"name": "local-debug.json", "reason": "debug"}
            )
            manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_upload_manifest(out_dir, check=False)

            self.assertIn("non-public workflow artifact set mismatch", result.stderr)
            self.assertIn("local-debug.json", result.stderr)

    def test_release_publish_receipt_writes_and_validates_publish_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)

            result = run_release_publish_receipt(out_dir, "--write")
            receipt = json.loads((out_dir / PUBLISH_RECEIPT_NAME).read_text())
            manifest = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(receipt["schema_version"], 2)
            self.assertEqual(receipt["kind"], "m80_release_publish_decision")
            self.assertEqual(receipt["decision"], "approved")
            self.assertEqual(receipt["release_tag"], "v0.2.9")
            self.assertEqual(receipt["commit_sha"], INTEGRITY_COMMIT_SHA)
            self.assertEqual(receipt["workflow_run_id"], "12345")
            self.assertEqual(receipt["actor"], "release-bot")
            self.assertEqual(receipt["repository"], "moradology/m80")
            self.assertEqual(receipt["github_ref"], "refs/tags/v0.2.9")
            self.assertEqual(receipt["artifact_manifest"]["name"], UPLOAD_MANIFEST_NAME)
            self.assertEqual(receipt["artifact_manifest_digest"], receipt["artifact_manifest"]["sha256"])
            self.assertEqual(receipt["proof_ledger"]["name"], RELEASE_PROOF_LEDGER_NAME)
            self.assertEqual(receipt["proof_ledger_digest"], receipt["proof_ledger"]["sha256"])
            self.assertEqual(
                receipt["quickstart_proofs"],
                [
                    {
                        "lane_id": "hostless-quickstart",
                        "proof_kind": "quickstart-proof",
                        "substrate": "hostless",
                        "file": {
                            "name": HOSTLESS_QUICKSTART_PROOF_NAME,
                            "sha256": f"sha256:{sha256(out_dir / HOSTLESS_QUICKSTART_PROOF_NAME)}",
                            "size_bytes": (out_dir / HOSTLESS_QUICKSTART_PROOF_NAME).stat().st_size,
                        },
                    }
                ],
            )
            self.assertEqual(
                {asset["name"] for asset in receipt["public_assets"]},
                {asset["name"] for asset in manifest["public_assets"]},
            )

    def test_release_publish_receipt_rejects_missing_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)

            result = run_release_publish_receipt(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release publish decision receipt missing", result.stderr)

    def test_release_publish_receipt_rejects_stale_artifact_manifest_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)
            run_release_publish_receipt(out_dir, "--write")
            manifest_path = out_dir / UPLOAD_MANIFEST_NAME
            manifest = json.loads(manifest_path.read_text())
            manifest["non_public_workflow_artifacts"][0]["reason"] = "tampered"
            manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")

            result = run_release_publish_receipt(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("artifact manifest sha256 mismatch", result.stderr)

    def test_release_publish_receipt_rejects_stale_proof_ledger_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            proof = write_publish_proof_ledger(out_dir)
            run_release_publish_receipt(out_dir, "--write")
            proof.write_text("{\"tampered\": true}\n")

            result = run_release_publish_receipt(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof ledger sha256 mismatch", result.stderr)

    def test_release_publish_receipt_rejects_ledger_proof_json_swap(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)

            result = run_release_publish_receipt(
                out_dir,
                "--write",
                "--proof-ledger",
                str(out_dir / HOSTLESS_QUICKSTART_PROOF_NAME),
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof_ledger must not reference quickstart proof JSON", result.stderr)

    def test_release_publish_receipt_rejects_missing_quickstart_proof_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)
            (out_dir / HOSTLESS_QUICKSTART_PROOF_NAME).unlink()

            result = run_release_publish_receipt(out_dir, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release publish hostless quickstart proof missing", result.stderr)

    def test_release_publish_receipt_rejects_stale_quickstart_proof_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)
            run_release_publish_receipt(out_dir, "--write")
            (out_dir / HOSTLESS_QUICKSTART_PROOF_NAME).write_text("{\"tampered\": true}\n")

            result = run_release_publish_receipt(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("quickstart proof hostless-quickstart sha256 mismatch", result.stderr)

    def test_release_publish_receipt_rejects_duplicate_quickstart_proof_file_refs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)

            result = run_release_publish_receipt(
                out_dir,
                "--write",
                "--real-kvm-proof",
                str(out_dir / HOSTLESS_QUICKSTART_PROOF_NAME),
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate quickstart proof file", result.stderr)

    def test_release_publish_receipt_rejects_legacy_ambiguous_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)
            run_release_publish_receipt(out_dir, "--write")
            receipt_path = out_dir / PUBLISH_RECEIPT_NAME
            receipt = json.loads(receipt_path.read_text())
            receipt["schema_version"] = 1
            receipt["proof_ledger"] = receipt["quickstart_proofs"][0]["file"]
            del receipt["quickstart_proofs"]
            receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")

            result = run_release_publish_receipt(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("field mismatch", result.stderr)

    def test_release_publish_receipt_rejects_wrong_actor_context(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)
            run_release_publish_receipt(out_dir, "--write", actor="release-bot")

            result = run_release_publish_receipt(out_dir, actor="other-actor", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("actor mismatch", result.stderr)

    def test_release_publish_receipt_rejects_wrong_ref_context(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = release_upload_manifest_fixture(Path(tmp))
            write_publish_proof_ledger(out_dir)
            run_release_publish_receipt(out_dir, "--write")

            result = run_release_publish_receipt(
                out_dir,
                github_ref="refs/heads/main",
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("github-ref must be the release tag ref", result.stderr)

    def test_release_evidence_bundle_writes_schema_entrypoint(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            bundle = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            manifest = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())

            self.assertEqual(bundle["schema_version"], 2)
            self.assertEqual(bundle["kind"], "m80_release_evidence_bundle")
            self.assertEqual(bundle["release_tag"], "v0.2.9")
            self.assertEqual(bundle["commit_sha"], INTEGRITY_COMMIT_SHA)
            self.assertEqual(bundle["workflow_run_id"], "12345")
            self.assertEqual(bundle["m80_version"], "v0.2.9")
            self.assertEqual(bundle["resolved_install_tag"], "v0.2.9")
            self.assertEqual(bundle["upload_manifest"]["name"], UPLOAD_MANIFEST_NAME)
            self.assertEqual(bundle["build_handoff"]["name"], BUILD_MANIFEST_NAME)
            self.assertEqual(bundle["publish_decision_receipt"]["name"], PUBLISH_RECEIPT_NAME)
            self.assertEqual(bundle["proof_ledger"]["name"], RELEASE_PROOF_LEDGER_NAME)
            self.assertIn("real-kvm-quickstart", bundle["required_lane_ids"])
            self.assertIn("real-kvm-quickstart", bundle["missing_required_lane_ids"])
            self.assertNotIn("hostless-quickstart", bundle["missing_required_lane_ids"])
            self.assertEqual(
                {asset["name"] for asset in bundle["public_assets"]},
                {asset["name"] for asset in manifest["public_assets"]},
            )
            workflow_only_names = {artifact["name"] for artifact in bundle["workflow_only_artifacts"]}
            self.assertIn(UPLOAD_MANIFEST_NAME, workflow_only_names)
            self.assertIn(HOSTLESS_QUICKSTART_PROOF_NAME, workflow_only_names)
            self.assertIn(RELEASE_PROOF_LEDGER_NAME, workflow_only_names)
            hostless_proof_ref = {
                "name": HOSTLESS_QUICKSTART_PROOF_NAME,
                "sha256": f"sha256:{sha256(out_dir / HOSTLESS_QUICKSTART_PROOF_NAME)}",
                "size_bytes": (out_dir / HOSTLESS_QUICKSTART_PROOF_NAME).stat().st_size,
            }
            self.assertEqual(
                bundle["proofs"],
                [
                    {
                        "artifact_class": "workflow-only",
                        "file": hostless_proof_ref,
                        "lane_id": "hostless-quickstart",
                        "proof_kind": "quickstart-proof",
                        "substrate": "hostless",
                    }
                ],
            )
            self.assertIn("absolute-host-paths", bundle["redaction"]["forbidden"])

    def test_release_evidence_bundle_rejects_proof_row_ledger_swap(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload["proofs"][0]["file"] = payload["proof_ledger"]
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof hostless-quickstart file mismatch", result.stderr)

    def test_release_evidence_bundle_rejects_missing_quickstart_proof_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            (out_dir / HOSTLESS_QUICKSTART_PROOF_NAME).unlink()

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("workflow-only artifact missing: m80-quickstart-proof-hostless.json", result.stderr)

    def test_release_evidence_bundle_rejects_stale_quickstart_proof_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            proof_path = out_dir / HOSTLESS_QUICKSTART_PROOF_NAME
            proof_path.write_text("{\"tampered\": true}\n")
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            for artifact in payload["workflow_only_artifacts"]:
                if artifact["name"] == HOSTLESS_QUICKSTART_PROOF_NAME:
                    artifact["sha256"] = sha256(proof_path)
                    artifact["size_bytes"] = proof_path.stat().st_size
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof hostless-quickstart file mismatch", result.stderr)

    def test_release_evidence_bundle_rejects_duplicate_proof_file_refs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload["proofs"].append(
                {
                    "artifact_class": "workflow-only",
                    "file": payload["proofs"][0]["file"],
                    "lane_id": "workflow-policy",
                    "proof_kind": "workflow-policy-report",
                    "substrate": "github-actions",
                }
            )
            payload["missing_required_lane_ids"].remove("workflow-policy")
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate proof file: m80-quickstart-proof-hostless.json", result.stderr)

    def test_release_evidence_bundle_rejects_stale_proof_ledger_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            (out_dir / RELEASE_PROOF_LEDGER_NAME).write_text("{\"tampered\": true}\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof ledger mismatch", result.stderr)

    def test_release_evidence_bundle_rejects_missing_required_key(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload.pop("commit_sha")
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("field mismatch: missing commit_sha", result.stderr)

    def test_release_evidence_bundle_rejects_unknown_schema_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload["schema_version"] = 999
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported release evidence bundle schema_version", result.stderr)

    def test_release_evidence_bundle_rejects_duplicate_lane_ids(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload["required_lane_ids"].append(payload["required_lane_ids"][0])
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("required_lane_ids duplicate lane id", result.stderr)

    def test_release_evidence_bundle_rejects_unaccounted_required_lane(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload["missing_required_lane_ids"].remove("real-kvm-quickstart")
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("required lane coverage mismatch", result.stderr)

    def test_release_evidence_bundle_rejects_malformed_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            payload["upload_manifest"]["sha256"] = "not-a-digest"
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("upload manifest sha256 must be sha256:<lowercase digest>", result.stderr)

    def test_release_evidence_bundle_rejects_public_workflow_artifact_confusion(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            public_asset = payload["public_assets"][0]
            payload["workflow_only_artifacts"].append(
                {
                    "name": public_asset["name"],
                    "reason": "bad overlap",
                    "sha256": public_asset["sha256"],
                    "size_bytes": public_asset["size_bytes"],
                }
            )
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public/workflow artifact confusion", result.stderr)

    def test_release_evidence_bundle_rejects_hostless_as_real_kvm_proof(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = evidence_bundle_fixture(Path(tmp))
            payload = json.loads((out_dir / EVIDENCE_BUNDLE_NAME).read_text())
            proof = dict(payload["proofs"][0])
            proof["lane_id"] = "real-kvm-quickstart"
            proof["substrate"] = "hostless"
            payload["proofs"] = [proof]
            (out_dir / EVIDENCE_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_release_evidence_bundle(out_dir, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof real-kvm-quickstart substrate mismatch", result.stderr)

    def test_remote_release_asset_inventory_writes_remote_byte_digests(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            metadata = write_remote_release_metadata(out_dir, redownload)
            assets_metadata = write_remote_release_assets_metadata(metadata)

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", assets_metadata=assets_metadata)
            inventory = json.loads((redownload / REMOTE_INVENTORY_NAME).read_text())
            manifest = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(inventory["schema_version"], 1)
            self.assertEqual(inventory["kind"], "m80_release_remote_asset_inventory")
            self.assertEqual(inventory["release_tag"], "v0.2.9")
            self.assertEqual(inventory["release_id"], 9001)
            self.assertEqual(
                {asset["name"] for asset in inventory["assets"]},
                {asset["name"] for asset in manifest["public_assets"]},
            )
            for asset in inventory["assets"]:
                self.assertEqual(asset["sha256"], sha256(redownload / asset["name"]))
                self.assertEqual(asset["size_bytes"], (redownload / asset["name"]).stat().st_size)
                self.assertTrue(asset["download_url"].endswith(f"/releases/download/v0.2.9/{asset['name']}"))
                self.assertIn("created_at", asset)
                self.assertIn("updated_at", asset)

    def test_remote_release_asset_inventory_rejects_duplicate_remote_name(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            metadata = write_remote_release_metadata(out_dir, redownload, duplicate_first=True)

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate asset name", result.stderr)

    def test_remote_release_asset_inventory_rejects_missing_remote_downloaded_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            metadata = write_remote_release_metadata(out_dir, redownload)
            missing = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())["public_assets"][0]["name"]
            (redownload / missing).unlink()

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"downloaded bytes missing: {missing}", result.stderr)

    def test_remote_release_asset_inventory_rejects_stale_local_only_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            omitted = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())["public_assets"][0]["name"]
            metadata = write_remote_release_metadata(out_dir, redownload, omit_name=omitted)

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("remote release asset metadata set mismatch", result.stderr)
            self.assertIn(omitted, result.stderr)

    def test_remote_release_asset_inventory_rejects_remote_digest_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            metadata = write_remote_release_metadata(out_dir, redownload)
            stale = json.loads((out_dir / UPLOAD_MANIFEST_NAME).read_text())["public_assets"][0]["name"]
            stale_path = redownload / stale
            stale_path.write_bytes(b"x" * stale_path.stat().st_size)
            refresh_remote_release_metadata_size(metadata, stale, (redownload / stale).stat().st_size)

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"remote release asset {stale} sha256 mismatch", result.stderr)

    def test_remote_release_asset_inventory_rejects_missing_download_url(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            metadata = write_remote_release_metadata(out_dir, redownload)
            payload = json.loads(metadata.read_text())
            missing = payload["assets"][0]["name"]
            payload["assets"][0].pop("browser_download_url")
            metadata.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"asset {missing} browser_download_url", result.stderr)

    def test_remote_release_asset_inventory_rejects_duplicate_remote_id(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out_dir = release_upload_manifest_fixture(root)
            redownload = root / "redownload"
            copy_manifest_public_assets(out_dir, redownload)
            metadata = write_remote_release_metadata(out_dir, redownload)
            payload = json.loads(metadata.read_text())
            payload["assets"][1]["id"] = payload["assets"][0]["id"]
            metadata.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

            result = run_remote_asset_inventory(redownload, out_dir, metadata, "--write", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate asset id", result.stderr)

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
            inputs = fixture_inputs(root, release_tag="v0.2.9")
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

    def test_rejects_binary_source_commit_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            write_fake_m80(inputs["m80"], "v0.2.9", source_commit="1" * 40)

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80 source_commit != release commit", result.stderr)

    def test_rejects_binary_target_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            write_fake_m80(inputs["m80"], "v0.2.9", target="linux-aarch64")

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80 target != release target", result.stderr)

    def test_rejects_binary_target_triple_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            write_fake_m80(inputs["m80"], "v0.2.9", target_triple="aarch64-unknown-linux-gnu")

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80 target_triple missing from release target_triples", result.stderr)

    def test_rejects_workspace_release_tag_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            result = run_package(inputs, root / "out", release_tag="v9.9.9", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release tag mismatch", result.stderr)

    def test_rejects_prerelease_release_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            result = run_package(inputs, root / "out", release_tag="v0.2.9-rc.1", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("stable release tag must be vMAJOR.MINOR.PATCH", result.stderr)

    def test_rejects_guest_protocol_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9", guest_protocol=2)
            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-guestd protocol_version != m80 protocol_version", result.stderr)

    def test_rejects_manifest_schema_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            rewrite_manifest(inputs["manifest"], {"schema_version": 99})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("guest manifest schema_version mismatch", result.stderr)

    def test_rejects_build_receipt_schema_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            rewrite_json(inputs["receipt"], {"schema_version": 99})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt schema_version mismatch", result.stderr)

    def test_rejects_guest_manifest_image_kind_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            rewrite_manifest(inputs["manifest"], {"image_kind": "ubuntu"})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("guest manifest image_kind mismatch", result.stderr)

    def test_rejects_unsupported_package_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")

            result = run_package(inputs, root / "out", target="linux-arm64", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported target", result.stderr)

    def test_rejects_unsupported_package_image_kind(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")

            result = run_package(inputs, root / "out", image_kind="ubuntu", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported image kind", result.stderr)

    def test_rejects_build_receipt_manifest_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            rewrite_json(inputs["receipt"], {"manifest_sha256": "0" * 64})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt manifest_sha256 mismatch", result.stderr)

    def test_rejects_build_receipt_manifest_path_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            rewrite_json(inputs["receipt"], {"manifest_path": str(root / "other.manifest.json")})

            result = run_package(inputs, root / "out", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build receipt manifest_path mismatch", result.stderr)

    def test_rejects_build_receipt_artifact_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
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
            (f"{BUILD_MANIFEST_NAME}.sha256", BUILD_MANIFEST_NAME, "checksum sidecar mismatch"),
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

    def test_verifier_accepts_downloaded_public_assets_without_posix_modes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            (root / "out" / "install.sh").chmod(0o644)

            run_verify(
                tarball,
                verify_sidecars=True,
                downloaded_public_assets=True,
            )

    def test_verifier_rejects_build_manifest_metadata_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_build_manifest(root / "out", {"bundle_metadata_sha256": "0" * 64})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest bundle_metadata_sha256 mismatch", result.stderr)

    def test_verifier_rejects_build_manifest_commit_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_build_manifest(root / "out", {"source_commit": "1" * 40})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest source_commit/integrity commit_sha mismatch", result.stderr)

    def test_verifier_rejects_build_manifest_cargo_lock_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_build_manifest(root / "out", {"cargo_lock_sha256": "0" * 64})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest cargo_lock_sha256 mismatch", result.stderr)

    def test_verifier_rejects_build_manifest_without_builder_material(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_build_manifest(root / "out", {"apt_packages": [], "container_digest": None})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest missing apt packages or container digest", result.stderr)

    def test_verifier_rejects_build_manifest_malformed_container_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            rewrite_build_manifest(root / "out", {"container_digest": "sha256:" + ("0" * 63)})

            result = run_verify(tarball, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest container_digest invalid", result.stderr)

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

    def test_release_integrity_contract_fixture_matches_python_verifier(self) -> None:
        contract = load_release_integrity_contract()
        verifier = runpy.run_path(str(VERIFY_INTEGRITY))

        self.assertEqual(contract["schema_version"], verifier["SCHEMA_VERSION"])
        self.assertEqual(contract["mechanism"], verifier["MECHANISM"])
        self.assertEqual(contract["repository"], verifier["REPOSITORY"])
        self.assertEqual(contract["target"], verifier["TARGET"])
        self.assertEqual(contract["default_bundle"]["name"], verifier["BUNDLE_NAME"])
        self.assertEqual(contract["default_bundle"]["checksum_name"], f"{BUNDLE_NAME}.sha256")
        self.assertEqual(contract["default_bundle"]["metadata_name"], verifier["METADATA_NAME"])
        self.assertEqual(
            contract["default_bundle"]["metadata_checksum_name"],
            f"{METADATA_NAME}.sha256",
        )
        self.assertEqual(contract["required_files"]["asset_index"], verifier["ASSET_INDEX_NAME"])
        self.assertEqual(
            contract["required_files"]["bootstrap_selector"],
            verifier["BOOTSTRAP_SELECTOR_NAME"],
        )
        self.assertEqual(contract["required_files"]["build_manifest"], verifier["BUILD_MANIFEST_NAME"])
        self.assertEqual(contract["required_files"]["install"], verifier["INSTALL_NAME"])
        self.assertEqual(contract["required_files"]["integrity_predicate"], INTEGRITY_NAME)
        self.assertEqual(contract["required_files"]["public_sha256s"], "SHA256SUMS")
        self.assertEqual(
            contract["attestation"]["bundle_name"],
            verifier["INTEGRITY_ATTESTATION_BUNDLE_NAME"],
        )
        self.assertEqual(contract["attestation"]["metadata_name"], INTEGRITY_ATTESTATION_METADATA_NAME)
        self.assertEqual(contract["attestation"]["signer_workflow"], INTEGRITY_SIGNER_IDENTITY)
        self.assertEqual(contract["attestation"]["issuer"], INTEGRITY_SIGNER_ISSUER)
        self.assertEqual(contract["attestation"]["keyset_id"], INTEGRITY_KEYSET_ID)

        default_asset = {
            "name": BUNDLE_NAME,
            "checksum_name": f"{BUNDLE_NAME}.sha256",
            "metadata_name": METADATA_NAME,
            "signature_name": None,
        }
        expected_subjects = {
            role["name"]: role["subject_kind"]
            for role in contract["material_roles"]
            if role["subject_kind"] is not None
        }
        self.assertEqual(
            verifier["expected_subjects_from_index"]({"assets": [default_asset]}),
            expected_subjects,
        )
        self.assertEqual(
            verifier["expected_public_sha256s"](expected_subjects),
            {
                role["name"]: role["subject_kind"]
                for role in contract["material_roles"]
                if role["public_sha256s"]
            },
        )

        trust_policy = json.loads((REPO_ROOT / "docs/behaviors/release/m80-release-trust-policy.json").read_text())
        self.assertEqual(trust_policy["schema_version"], contract["schema_version"])
        self.assertEqual(trust_policy["mechanism"], contract["mechanism"])
        self.assertEqual(trust_policy["repository"], contract["repository"])
        self.assertEqual(trust_policy["allowed_signers"][0]["identity"], contract["attestation"]["signer_workflow"])
        self.assertEqual(trust_policy["allowed_signers"][0]["issuer"], contract["attestation"]["issuer"])
        self.assertEqual(trust_policy["keyset_id"], contract["attestation"]["keyset_id"])

    def test_human_release_dist_verifier_accepts_clean_public_dist(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            write_integrity_material(root / "out")

            result = run_verify(tarball, verify_integrity=True)

            self.assertIn("verified release integrity material", result.stdout)
            self.assertIn(f"verified {tarball}", result.stdout)

    def test_human_release_dist_verifier_rejects_tampered_tarball(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            write_integrity_material(root / "out")
            with tarball.open("ab") as f:
                f.write(b"tampered")

            result = run_verify(tarball, verify_integrity=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"checksum sidecar mismatch for {BUNDLE_NAME}", result.stderr)

    def test_human_release_dist_verifier_rejects_tampered_install_sh(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            write_integrity_material(root / "out")
            with (root / "out" / INSTALL_NAME).open("a") as f:
                f.write("\n# tampered\n")

            result = run_verify(tarball, verify_integrity=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"checksum sidecar mismatch for {INSTALL_NAME}", result.stderr)

    def test_human_release_dist_verifier_rejects_wrong_tag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            write_integrity_material(root / "out")

            result = run_verify(tarball, verify_integrity=True, release_tag="v9.9.9", check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release tag mismatch: expected v0.2.9, got v9.9.9", result.stderr)

    def test_human_release_dist_verifier_rejects_missing_attestation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            write_integrity_material(root / "out")
            (root / "out" / INTEGRITY_ATTESTATION_BUNDLE_NAME).unlink()

            result = run_verify(tarball, verify_integrity=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release attestation bundle missing", result.stderr)

    def test_human_release_dist_verifier_rejects_missing_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tarball = package_fixture(root)
            write_integrity_material(root / "out")
            (root / "out" / f"{INSTALL_NAME}.sha256").unlink()

            result = run_verify(tarball, verify_integrity=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"missing checksum sidecar: {INSTALL_NAME}.sha256", result.stderr)

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
            material = write_integrity_material(root / "out", omit_subject=signature.name)

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity missing subject(s): {signature.name}", result.stderr)

    def test_release_integrity_material_rejects_absent_asset_index_signature_ref(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            signature = root / "out" / "missing.sig"
            signature.write_text("signature\n")
            rewrite_asset_index_asset(root / "out", {"signature_name": signature.name})
            rewrite_bootstrap_selector_asset_field(root / "out", "signature_name", signature.name)
            material = write_integrity_material(root / "out")
            signature.unlink()

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
            material = write_integrity_material(root / "out")

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
            self.assertEqual(payload["release_tag"], "v0.2.9")
            self.assertEqual(payload["predicate_sha256"], sha256(material))
            self.assertEqual(payload["signer_identity"], INTEGRITY_SIGNER_IDENTITY)
            self.assertEqual(payload["issuer"], INTEGRITY_SIGNER_ISSUER)
            self.assertEqual(payload["keyset_id"], INTEGRITY_KEYSET_ID)
            self.assertEqual(payload["certificate_not_before"], "2026-01-01T00:00:00Z")
            self.assertEqual(payload["certificate_not_after"], "2027-01-01T00:00:00Z")
            run_verify_integrity(material)

    def test_release_integrity_material_accepts_multi_row_asset_index_subjects(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            add_alternate_image_kind_fixture(root / "out")
            material = write_integrity_material(root / "out")
            payload = json.loads(material.read_text())
            subject_names = {subject["name"] for subject in payload["subjects"]}

            self.assertIn("m80-linux-x86_64-debug.tar.gz", subject_names)
            self.assertIn("m80-linux-x86_64-debug.tar.gz.sha256", subject_names)
            self.assertIn("m80-linux-x86_64-debug.bundle.json", subject_names)
            self.assertIn("m80-linux-x86_64-debug.bundle.json.sha256", subject_names)
            run_verify_integrity(material)

    def test_release_integrity_material_rejects_missing_non_default_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            add_alternate_image_kind_fixture(root / "out")
            material = write_integrity_material(root / "out")
            (root / "out" / "m80-linux-x86_64-debug.tar.gz").unlink()

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index bundle missing", result.stderr)

    def test_release_integrity_material_rejects_non_default_asset_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            add_alternate_image_kind_fixture(root / "out")
            index = json.loads((root / "out" / ASSET_INDEX_NAME).read_text())
            for asset in index["assets"]:
                if asset["image_kind"] == "debug":
                    asset["sha256"] = "0" * 64
            rewrite_asset_index(root / "out", index)
            rewrite_bootstrap_selector(root / "out", bootstrap_selector_lines_for_index(root / "out", index))
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity asset index sha256 mismatch for m80-linux-x86_64-debug.tar.gz", result.stderr)

    def test_release_integrity_material_rejects_non_default_metadata_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            add_alternate_image_kind_fixture(root / "out")
            index = json.loads((root / "out" / ASSET_INDEX_NAME).read_text())
            for asset in index["assets"]:
                if asset["image_kind"] == "debug":
                    asset["metadata_sha256"] = "0" * 64
            rewrite_asset_index(root / "out", index)
            rewrite_bootstrap_selector(root / "out", bootstrap_selector_lines_for_index(root / "out", index))
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "release integrity asset index metadata_sha256 mismatch for m80-linux-x86_64-debug.tar.gz",
                result.stderr,
            )

    def test_release_integrity_material_rejects_non_default_selector_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            add_alternate_image_kind_fixture(root / "out")
            lines = bootstrap_selector_lines(root / "out")
            columns = lines[2].split("\t")
            size_index = columns.index("size_bytes")
            debug_rows = bootstrap_selector_rows_for_image_kind(lines, "debug")
            self.assertEqual(len(debug_rows), 1)
            row_parts = debug_rows[0].split("\t")
            row_parts[size_index] = "1"
            for index, line in enumerate(lines):
                if line == debug_rows[0]:
                    lines[index] = "\t".join(row_parts)
            rewrite_bootstrap_selector(root / "out", lines)
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity bootstrap selector size_bytes mismatch for linux/x86_64/debug", result.stderr)

    def test_release_integrity_material_rejects_public_sums_missing_non_default_asset(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            add_alternate_image_kind_fixture(root / "out")
            sums_path = root / "out" / "SHA256SUMS"
            lines = [
                line
                for line in sums_path.read_text().splitlines()
                if not line.endswith("  m80-linux-x86_64-debug.tar.gz")
            ]
            sums_path.write_text("\n".join(lines) + "\n")
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity public SHA256SUMS missing asset(s): m80-linux-x86_64-debug.tar.gz", result.stderr)

    def test_verifier_rejects_non_default_tar_internal_corruption_after_dist_integrity_refresh(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            seed_out = root / "seed"
            out_dir = root / "out"
            run_package(inputs, seed_out)
            manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
            run_package(inputs, out_dir, extra_tuple_manifests=[manifest])
            rewrite_release_tuple_bundle(
                out_dir,
                image_kind="debug",
                payload_updates={"artifacts/m80-guestd": lambda _data: b"tampered guestd\n"},
            )
            material = write_integrity_material(out_dir)

            run_verify_integrity(material)
            result = run_verify(out_dir / BUNDLE_NAME, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "asset index tuple linux/x86_64/debug bundle m80-linux-x86_64-debug.tar.gz failed verification",
                result.stderr,
            )
            self.assertIn("guest manifest daemon_binary_sha256 mismatch", result.stderr)

    def test_verifier_checks_every_extra_tuple_bundle_after_dist_integrity_refresh(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.2.9")
            seed_out = root / "seed"
            out_dir = root / "out"
            run_package(inputs, seed_out)
            debug_manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
            trace_manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="trace")
            run_package(inputs, out_dir, extra_tuple_manifests=[debug_manifest, trace_manifest])
            rewrite_release_tuple_bundle(
                out_dir,
                image_kind="trace",
                payload_updates={"artifacts/m80-guestd": lambda _data: b"tampered trace guestd\n"},
            )
            material = write_integrity_material(out_dir)

            run_verify_integrity(material)
            result = run_verify(out_dir / BUNDLE_NAME, verify_sidecars=True, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "asset index tuple linux/x86_64/trace bundle m80-linux-x86_64-trace.tar.gz failed verification",
                result.stderr,
            )
            self.assertIn("guest manifest daemon_binary_sha256 mismatch", result.stderr)

    def test_verifier_rejects_non_flat_extra_tuple_asset_names_before_path_lookup(self) -> None:
        cases = [
            ("name", "../m80-linux-x86_64-debug.tar.gz", "asset index name"),
            ("metadata_name", "../m80-linux-x86_64-debug.bundle.json", "asset index metadata_name"),
            ("checksum_name", "../m80-linux-x86_64-debug.tar.gz.sha256", "asset index checksum_name"),
            ("signature_name", "../m80-linux-x86_64-debug.tar.gz.sig", "asset index signature_name"),
            ("attestation_name", "../m80-release-integrity.attestation.jsonl", "asset index attestation_name"),
        ]
        for field, value, label in cases:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                inputs = fixture_inputs(root, release_tag="v0.2.9")
                seed_out = root / "seed"
                out_dir = root / "out"
                run_package(inputs, seed_out)
                manifest = write_extra_tuple_manifest_from_seed(root, seed_out, image_kind="debug")
                run_package(inputs, out_dir, extra_tuple_manifests=[manifest])
                index_path = out_dir / ASSET_INDEX_NAME
                index = json.loads(index_path.read_text())
                for asset in index["assets"]:
                    if asset["image_kind"] == "debug":
                        asset[field] = value
                index_path.write_text(json.dumps(index, indent=2, sort_keys=True) + "\n")
                write_sha256_sidecar(out_dir / f"{ASSET_INDEX_NAME}.sha256", index_path, ASSET_INDEX_NAME)

                result = run_verify(out_dir / BUNDLE_NAME, verify_sidecars=True, check=False)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"release dist asset name must be flat for {label}", result.stderr)
                self.assertIn(value, result.stderr)

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
                    BUILD_MANIFEST_NAME,
                    f"{BUILD_MANIFEST_NAME}.sha256",
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

    def test_release_integrity_material_rejects_missing_build_manifest_subject(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            material = write_integrity_material(root / "out", omit_subject=BUILD_MANIFEST_NAME)

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f"release integrity missing subject(s): {BUILD_MANIFEST_NAME}", result.stderr)

    def test_release_integrity_material_rejects_build_manifest_commit_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"source_commit": "1" * 40})
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release integrity build manifest source_commit mismatch", result.stderr)

    def test_release_integrity_material_rejects_build_manifest_malformed_container_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_fixture(root)
            rewrite_build_manifest(root / "out", {"container_digest": "sha256:" + ("A" * 64)})
            material = write_integrity_material(root / "out")

            result = run_verify_integrity(material, check=False)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("build manifest container_digest invalid", result.stderr)

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
            self.assertIn(f"release integrity public SHA256SUMS hash mismatch for {INSTALL_NAME}", result.stderr)

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
                    "signer_identity": "repo:moradology/other:ref:refs/tags/v0.2.9",
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


def fake_m80_payload(
    release_tag: str = "v0.2.9",
    *,
    updates: dict | None = None,
    omit: set[str] | None = None,
) -> dict:
    payload = {
        "version": 1,
        "data": {
            "binary_version": release_tag,
            "package_version": "0.2.9",
            "release_tag": release_tag,
            "release_build": True,
            "version_status": "release",
            "expected_release_tag": "v0.2.9",
            "source_commit": INTEGRITY_COMMIT_SHA,
            "target": RELEASE_TARGET,
            "target_triple": RELEASE_TARGET_TRIPLE,
            "protocol_version": 1,
            "manifest_schema_version": 5,
            "build_receipt_schema_version": 1,
            "install_provenance_schema_version": 1,
            "firecracker_pin": "unknown",
        },
    }
    if updates:
        payload["data"].update(updates)
    for field in omit or set():
        payload["data"].pop(field, None)
    return payload


def fake_m80_script(payload: dict) -> str:
    rendered = shlex.quote(json.dumps(payload))
    return (
        "#!/bin/sh\n"
        "if [ \"${1:-}\" = install ]; then\n"
        "  if [ -n \"${M80_FAKE_INSTALL_ARGS:-}\" ]; then\n"
        "    : > \"$M80_FAKE_INSTALL_ARGS\"\n"
        "    for arg in \"$@\"; do printf '%s\\n' \"$arg\" >> \"$M80_FAKE_INSTALL_ARGS\"; done\n"
        "  fi\n"
        "  exit 0\n"
        "fi\n"
        f"printf '%s\\n' {rendered}\n"
    )


def write_fake_m80(
    path: Path,
    release_tag: str,
    *,
    source_commit: str = INTEGRITY_COMMIT_SHA,
    target: str = RELEASE_TARGET,
    target_triple: str = RELEASE_TARGET_TRIPLE,
) -> None:
    write_executable(
        path,
        fake_m80_script(
            fake_m80_payload(
                release_tag,
                updates={
                    "source_commit": source_commit,
                    "target": target,
                    "target_triple": target_triple,
                },
            )
        ),
    )


def write_fake_guestd(path: Path, protocol_version: int) -> None:
    write_executable(path, f"#!/bin/sh\nprintf 'm80-guestd 0.2.9 (proto v{protocol_version})\\n'\n")


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
    write_public_sha256s(out_dir / "SHA256SUMS", public_sha256_assets_for_index(out_dir))


def add_alternate_image_kind_fixture(out_dir: Path, *, image_kind: str = "debug") -> None:
    bundle_name = f"m80-linux-x86_64-{image_kind}.tar.gz"
    metadata_name = f"m80-linux-x86_64-{image_kind}.bundle.json"
    bundle_path = out_dir / bundle_name
    metadata_path = out_dir / metadata_name
    rewrite_tar(
        out_dir / BUNDLE_NAME,
        bundle_path,
        metadata_updates={"image_kind": image_kind},
    )
    metadata = json.loads((out_dir / METADATA_NAME).read_text())
    metadata["image_kind"] = image_kind
    metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    write_sha256_sidecar(out_dir / f"{bundle_name}.sha256", bundle_path, bundle_name)
    write_sha256_sidecar(out_dir / f"{metadata_name}.sha256", metadata_path, metadata_name)

    index_path = out_dir / ASSET_INDEX_NAME
    index = json.loads(index_path.read_text())
    asset = dict(index["assets"][0])
    asset.update(
        {
            "name": bundle_name,
            "url": f"https://github.com/moradology/m80/releases/download/v0.2.9/{bundle_name}",
            "sha256": sha256(bundle_path),
            "size_bytes": bundle_path.stat().st_size,
            "metadata_name": metadata_name,
            "metadata_sha256": sha256(metadata_path),
            "checksum_name": f"{bundle_name}.sha256",
            "image_kind": image_kind,
        }
    )
    index["assets"].append(asset)
    rewrite_asset_index(out_dir, index)
    rewrite_bootstrap_selector(out_dir, bootstrap_selector_lines_for_index(out_dir, index))


def write_extra_tuple_manifest_from_seed(root: Path, seed_out: Path, *, image_kind: str) -> Path:
    tuple_dir = root / "tuple-inputs"
    tuple_dir.mkdir(exist_ok=True)
    bundle_name = f"m80-linux-x86_64-{image_kind}.tar.gz"
    metadata_name = f"m80-linux-x86_64-{image_kind}.bundle.json"
    bundle_path = tuple_dir / bundle_name
    metadata_path = tuple_dir / metadata_name
    metadata = write_tuple_bundle_from_seed(seed_out / BUNDLE_NAME, bundle_path, image_kind=image_kind)
    metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    manifest = tuple_dir / f"{image_kind}-tuple.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "bundle_path": str(bundle_path),
                "metadata_path": str(metadata_path),
                "bundle_name": bundle_name,
                "metadata_name": metadata_name,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    return manifest


def write_tuple_bundle_from_seed(seed_bundle: Path, bundle_path: Path, *, image_kind: str) -> dict:
    entries: dict[str, tuple[bytes, int]] = {}
    with tarfile.open(seed_bundle, "r:gz") as tar:
        for member in tar.getmembers():
            if not member.isfile():
                continue
            data = tar.extractfile(member).read()  # type: ignore[union-attr]
            entries[member.name] = (data, member.mode & 0o777)

    guest_manifest_path = "artifacts/output.ext4.manifest.json"
    guest_manifest = json.loads(entries[guest_manifest_path][0].decode("utf-8"))
    guest_manifest["image_kind"] = image_kind
    guest_manifest_bytes = (json.dumps(guest_manifest, indent=2, sort_keys=True) + "\n").encode()
    entries[guest_manifest_path] = (guest_manifest_bytes, entries[guest_manifest_path][1])
    guest_manifest_sha = sha256_bytes(guest_manifest_bytes)

    receipt_path = "artifacts/output.ext4.build-receipt.json"
    receipt = json.loads(entries[receipt_path][0].decode("utf-8"))
    receipt["manifest_sha256"] = guest_manifest_sha
    receipt_bytes = (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode()
    entries[receipt_path] = (receipt_bytes, entries[receipt_path][1])

    metadata = json.loads(entries["bundle.json"][0].decode("utf-8"))
    metadata["image_kind"] = image_kind
    for row in metadata["files"]:
        if row["path"] == guest_manifest_path:
            row["sha256"] = guest_manifest_sha
        if row["path"] == receipt_path:
            row["sha256"] = sha256_bytes(receipt_bytes)
    metadata_bytes = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode()
    entries["bundle.json"] = (metadata_bytes, entries["bundle.json"][1])

    sums = parse_sha256sum_bytes(entries["SHA256SUMS"][0])
    sums[guest_manifest_path] = guest_manifest_sha
    sums[receipt_path] = sha256_bytes(receipt_bytes)
    sums["bundle.json"] = sha256_bytes(metadata_bytes)
    sums_text = "".join(f"{sums[name]}  {name}\n" for name in sorted(sums))
    entries["SHA256SUMS"] = (sums_text.encode(), entries["SHA256SUMS"][1])

    with tarfile.open(bundle_path, "w:gz") as tar:
        for name, (data, mode) in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            tar.addfile(info, fileobj=BytesReader(data))
    return metadata


def parse_sha256sum_bytes(data: bytes) -> dict[str, str]:
    sums: dict[str, str] = {}
    for line in data.decode("utf-8").splitlines():
        if not line.strip():
            continue
        digest, path = line.split(maxsplit=1)
        sums[path.strip()] = digest
    return sums


def bootstrap_selector_lines_for_index(out_dir: Path, index: dict) -> list[str]:
    lines = bootstrap_selector_lines(out_dir)[:3]
    columns = lines[2].split("\t")[1:]
    for asset in index["assets"]:
        lines.append(
            "row\t"
            + "\t".join(
                selector_value_for_test(asset[asset_index_field_for_selector(column)])
                for column in columns
            )
        )
    return lines


def asset_index_field_for_selector(column: str) -> str:
    return {
        "bundle_name": "name",
        "bundle_url": "url",
        "bundle_sha256": "sha256",
    }.get(column, column)


def selector_value_for_test(value: object) -> str:
    if value is None:
        return "-"
    return str(value)


def rewrite_build_manifest(out_dir: Path, updates: dict) -> None:
    manifest_path = out_dir / BUILD_MANIFEST_NAME
    manifest = json.loads(manifest_path.read_text())
    manifest.update(updates)
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    write_sha256_sidecar(out_dir / f"{BUILD_MANIFEST_NAME}.sha256", manifest_path, BUILD_MANIFEST_NAME)
    write_public_sha256s(out_dir / "SHA256SUMS", public_sha256_assets_for_index(out_dir))


def bootstrap_selector_lines(out_dir: Path) -> list[str]:
    return (out_dir / BOOTSTRAP_SELECTOR_NAME).read_text().splitlines()


def bootstrap_selector_rows_for_image_kind(lines: list[str], image_kind: str) -> list[str]:
    columns = lines[2].split("\t")
    image_kind_index = columns.index("image_kind")
    return [line for line in lines[3:] if line.split("\t")[image_kind_index] == image_kind]


def rewrite_bootstrap_selector(out_dir: Path, lines: list[str]) -> None:
    selector = out_dir / BOOTSTRAP_SELECTOR_NAME
    selector.write_text("\n".join(lines) + "\n")
    write_sha256_sidecar(out_dir / f"{BOOTSTRAP_SELECTOR_NAME}.sha256", selector, BOOTSTRAP_SELECTOR_NAME)
    write_public_sha256s(out_dir / "SHA256SUMS", public_sha256_assets_for_index(out_dir))


def rewrite_bootstrap_selector_asset_field(out_dir: Path, field: str, value: str | None) -> None:
    lines = bootstrap_selector_lines(out_dir)
    columns = lines[2].split("\t")
    row = lines[3].split("\t")
    row[columns.index(field)] = value if value is not None else "-"
    lines[3] = "\t".join(row)
    rewrite_bootstrap_selector(out_dir, lines)


def replace_bundle_m80_for_install(out_dir: Path, script_text: str) -> None:
    rewritten = out_dir / "rewritten-m80-bundle.tar.gz"
    rewrite_tar(
        out_dir / BUNDLE_NAME,
        rewritten,
        payload_updates={"bin/m80": lambda _data: script_text.encode()},
    )
    rewritten.replace(out_dir / BUNDLE_NAME)
    bundle_sha = sha256(out_dir / BUNDLE_NAME)
    bundle_size = (out_dir / BUNDLE_NAME).stat().st_size
    write_sha256_sidecar(out_dir / f"{BUNDLE_NAME}.sha256", out_dir / BUNDLE_NAME, BUNDLE_NAME)
    rewrite_asset_index_asset(out_dir, {"sha256": bundle_sha, "size_bytes": bundle_size})
    rewrite_bootstrap_selector_asset_field(out_dir, "bundle_sha256", bundle_sha)
    rewrite_bootstrap_selector_asset_field(out_dir, "size_bytes", str(bundle_size))
    write_public_sha256s(out_dir / "SHA256SUMS", public_sha256_assets_for_index(out_dir))
    write_integrity_material(out_dir)


def rewrite_release_tuple_bundle(out_dir: Path, *, image_kind: str, payload_updates: dict[str, object]) -> None:
    index = json.loads((out_dir / ASSET_INDEX_NAME).read_text())
    matching = [asset for asset in index["assets"] if asset["image_kind"] == image_kind]
    if len(matching) != 1:
        raise AssertionError(f"expected exactly one {image_kind} tuple")
    asset = matching[0]
    bundle_path = out_dir / asset["name"]
    rewritten = out_dir / f"rewritten-{asset['name']}"
    rewrite_tar(bundle_path, rewritten, payload_updates=payload_updates)
    rewritten.replace(bundle_path)
    bundle_path.chmod(0o644)
    asset["sha256"] = sha256(bundle_path)
    asset["size_bytes"] = bundle_path.stat().st_size
    rewrite_asset_index(out_dir, index)
    write_sha256_sidecar(out_dir / asset["checksum_name"], bundle_path, asset["name"])
    rewrite_bootstrap_selector(out_dir, bootstrap_selector_lines_for_index(out_dir, index))
    write_public_sha256s(out_dir / "SHA256SUMS", public_sha256_assets_for_index(out_dir))


def public_sha256_assets_for_index(out_dir: Path) -> list[tuple[str, Path]]:
    index = json.loads((out_dir / ASSET_INDEX_NAME).read_text())
    assets: list[tuple[str, Path]] = []
    for asset in index["assets"]:
        assets.append((asset["name"], out_dir / asset["name"]))
        assets.append((asset["checksum_name"], out_dir / asset["checksum_name"]))
    assets.extend(
        [
            (INSTALL_NAME, out_dir / INSTALL_NAME),
            (f"{INSTALL_NAME}.sha256", out_dir / f"{INSTALL_NAME}.sha256"),
        ]
    )
    for asset in index["assets"]:
        metadata_name = asset["metadata_name"]
        assets.append((metadata_name, out_dir / metadata_name))
        assets.append((f"{metadata_name}.sha256", out_dir / f"{metadata_name}.sha256"))
        if asset["signature_name"] is not None:
            assets.append((asset["signature_name"], out_dir / asset["signature_name"]))
    assets.extend(
        [
            (ASSET_INDEX_NAME, out_dir / ASSET_INDEX_NAME),
            (f"{ASSET_INDEX_NAME}.sha256", out_dir / f"{ASSET_INDEX_NAME}.sha256"),
            (BOOTSTRAP_SELECTOR_NAME, out_dir / BOOTSTRAP_SELECTOR_NAME),
            (f"{BOOTSTRAP_SELECTOR_NAME}.sha256", out_dir / f"{BOOTSTRAP_SELECTOR_NAME}.sha256"),
            (BUILD_MANIFEST_NAME, out_dir / BUILD_MANIFEST_NAME),
            (f"{BUILD_MANIFEST_NAME}.sha256", out_dir / f"{BUILD_MANIFEST_NAME}.sha256"),
        ]
    )
    return assets


def release_integrity_subject_kinds(out_dir: Path) -> dict[str, str]:
    subject_kinds = {
        INSTALL_NAME: "installer",
        f"{INSTALL_NAME}.sha256": "checksum-sidecar",
        ASSET_INDEX_NAME: "asset-index",
        f"{ASSET_INDEX_NAME}.sha256": "checksum-sidecar",
        BOOTSTRAP_SELECTOR_NAME: "bootstrap-selector",
        f"{BOOTSTRAP_SELECTOR_NAME}.sha256": "checksum-sidecar",
        BUILD_MANIFEST_NAME: "build-manifest",
        f"{BUILD_MANIFEST_NAME}.sha256": "checksum-sidecar",
        "SHA256SUMS": "checksum-manifest",
    }
    index = json.loads((out_dir / ASSET_INDEX_NAME).read_text())
    for asset in index["assets"]:
        subject_kinds[asset["name"]] = "release-bundle"
        subject_kinds[asset["checksum_name"]] = "checksum-sidecar"
        subject_kinds[asset["metadata_name"]] = "bundle-metadata"
        subject_kinds[f"{asset['metadata_name']}.sha256"] = "checksum-sidecar"
        if asset["signature_name"] is not None:
            subject_kinds[asset["signature_name"]] = "detached-signature"
    return subject_kinds


def write_integrity_material(
    out_dir: Path,
    *,
    updates: dict | None = None,
    omit_subject: str | None = None,
    omit_subject_field: tuple[str, str] | None = None,
    extra_subject: dict | None = None,
    subject_updates: dict[str, dict] | None = None,
) -> Path:
    subject_kinds = release_integrity_subject_kinds(out_dir)
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
        "release_tag": "v0.2.9",
        "commit_sha": INTEGRITY_COMMIT_SHA,
        "target": "linux-x86_64",
        "rust_toolchain": INTEGRITY_RUST_TOOLCHAIN,
        "m80_package_version": "0.2.9",
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
        "release_tag": "v0.2.9",
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
        "release_tag": "v0.2.9",
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
        "source_ref": "refs/tags/v0.2.9",
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
    curl_script: str | None = None,
) -> tuple[subprocess.CompletedProcess[str], list[str], Path]:
    out_dir = root / "out"
    fakebin = root / "fakebin-install"
    curl_log = root / "curl.log"
    curl_args_log = root / "curl-args.log"
    tar_log = root / "tar.log"
    install_args = root / "install-args.log"
    write_install_test_tools(fakebin)
    if curl_script is not None:
        write_executable(fakebin / "curl", curl_script)
    env = {
        "PATH": str(fakebin),
        "M80_RELEASE_ROOT": str(out_dir),
        "M80_CURL_LOG": str(curl_log),
        "M80_CURL_ARGS_LOG": str(curl_args_log),
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


def rendered_install_curl_arg_lines(root: Path) -> list[str]:
    path = root / "curl-args.log"
    return path.read_text().splitlines() if path.exists() else []


def write_install_test_tools(fakebin: Path) -> None:
    fakebin.mkdir()
    write_executable(
        fakebin / "curl",
        "#!/bin/sh\n"
        "argv=$*\n"
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
        "[ -z \"${M80_CURL_ARGS_LOG:-}\" ] || printf '%s\\n' \"$argv\" >> \"$M80_CURL_ARGS_LOG\"\n"
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
    "source_ref": "refs/tags/v0.2.9",
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
if bundle.get("release_tag") != "v0.2.9":
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
    bundle_url = f"https://github.com/moradology/m80/releases/download/v0.2.9/{BUNDLE_NAME}"
    test.assertNotIn(bundle_url, urls)
    test.assertFalse(install_args.exists())


def write_executable(path: Path, text: str) -> None:
    path.write_text(text)
    path.chmod(0o755)


def run_package(
    inputs: dict[str, Path],
    out_dir: Path,
    *,
    release_tag: str = "v0.2.9",
    target: str = "linux-x86_64",
    image_kind: str = "minimal",
    apt_package_versions: list[str] | None = None,
    container_digest: str | None = None,
    extra_tuple_manifests: list[Path] | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    if apt_package_versions is None:
        apt_package_versions = [
            "busybox-static=1.36.1",
            "curl=8.5.0",
            "e2fsprogs=1.47.0",
            "musl-tools=1.2.4",
        ]
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
        "--target-triple",
        "x86_64-unknown-linux-gnu",
        "--target-triple",
        "x86_64-unknown-linux-musl",
        "--builder-identity",
        "test-builder",
        "--builder-os-image",
        "test-os-image",
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
    for package_version in apt_package_versions:
        cmd.extend(["--apt-package-version", package_version])
    if container_digest is not None:
        cmd.extend(["--container-digest", container_digest])
    for manifest in extra_tuple_manifests or []:
        cmd.extend(["--extra-tuple-manifest", str(manifest)])
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def package_fixture(root: Path) -> Path:
    inputs = fixture_inputs(root, release_tag="v0.2.9")
    out_dir = root / "out"
    run_package(inputs, out_dir)
    return out_dir / "m80-linux-x86_64.tar.gz"


def package_signed_fixture(root: Path) -> Path:
    tarball = package_fixture(root)
    write_integrity_material(root / "out")
    return tarball


def release_upload_manifest_fixture(root: Path) -> Path:
    tarball = package_fixture(root)
    out_dir = tarball.parent
    write_integrity_material(out_dir)
    write_release_proof_ledger_placeholder(out_dir)
    run_release_upload_manifest(out_dir, "--write")
    return out_dir


def evidence_bundle_fixture(root: Path) -> Path:
    out_dir = release_upload_manifest_fixture(root)
    write_publish_proof_ledger(out_dir)
    run_release_publish_receipt(out_dir, "--write")
    run_release_evidence_bundle(out_dir, "--write")
    return out_dir


def write_publish_proof_ledger(out_dir: Path) -> Path:
    write_hostless_quickstart_proof_placeholder(out_dir)
    return write_release_proof_ledger_placeholder(out_dir)


def write_hostless_quickstart_proof_placeholder(out_dir: Path) -> Path:
    proof = out_dir / HOSTLESS_QUICKSTART_PROOF_NAME
    proof.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "kind": "m80_quickstart_proof",
                "proof_kind": "hostless",
                "release_tag": "v0.2.9",
                "substrate": {"kind": "hostless"},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    return proof


def write_release_proof_ledger_placeholder(out_dir: Path) -> Path:
    ledger = out_dir / RELEASE_PROOF_LEDGER_NAME
    ledger.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "record_hash": "sha256:" + ("a" * 64),
                "previous_record_hash": None,
                "proof_artifact": HOSTLESS_QUICKSTART_PROOF_NAME,
                "proof_artifact_digest": "sha256:" + ("b" * 64),
                "release_tag": "v0.2.9",
                "workflow_run_id": "12345",
                "proof_type": "hostless",
                "substrate": "hostless",
                "redaction": {
                    "environment": "omitted",
                    "host_paths": "omitted",
                    "secrets": "omitted",
                },
            },
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n"
    )
    return ledger


def copy_manifest_public_assets(source: Path, target: Path) -> None:
    target.mkdir()
    manifest = json.loads((source / UPLOAD_MANIFEST_NAME).read_text())
    for asset in manifest["public_assets"]:
        shutil.copy2(source / asset["name"], target / asset["name"])


def write_remote_release_metadata(
    manifest_dir: Path,
    redownload_dir: Path,
    *,
    omit_name: str | None = None,
    duplicate_first: bool = False,
) -> Path:
    manifest = json.loads((manifest_dir / UPLOAD_MANIFEST_NAME).read_text())
    assets = []
    for index, asset in enumerate(manifest["public_assets"], start=1):
        name = asset["name"]
        if name == omit_name:
            continue
        path = redownload_dir / name
        assets.append(
            {
                "id": 1000 + index,
                "name": name,
                "size": path.stat().st_size,
                "browser_download_url": f"https://github.com/moradology/m80/releases/download/v0.2.9/{name}",
                "created_at": "2026-05-21T00:00:00Z",
                "updated_at": "2026-05-21T00:00:01Z",
            }
        )
    if duplicate_first and assets:
        duplicate = dict(assets[0])
        duplicate["id"] = 9999
        assets.append(duplicate)
    metadata = {
        "id": 9001,
        "tag_name": "v0.2.9",
        "assets": assets,
    }
    path = redownload_dir / "github-release.json"
    path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    return path


def write_remote_release_assets_metadata(metadata_path: Path) -> Path:
    metadata = json.loads(metadata_path.read_text())
    path = metadata_path.with_name("github-release-assets.json")
    path.write_text(json.dumps([metadata["assets"]], indent=2, sort_keys=True) + "\n")
    return path


def refresh_remote_release_metadata_size(metadata_path: Path, name: str, size: int) -> None:
    metadata = json.loads(metadata_path.read_text())
    for asset in metadata["assets"]:
        if asset["name"] == name:
            asset["size"] = size
    metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")


def refresh_integrity_subject_and_manifest_asset(out_dir: Path, name: str) -> None:
    path = out_dir / name
    updates = {"sha256": sha256(path), "size_bytes": path.stat().st_size}
    material_path = out_dir / INTEGRITY_NAME
    material = json.loads(material_path.read_text())
    for subject in material["subjects"]:
        if subject["name"] == name:
            subject.update(updates)
    material_path.write_text(json.dumps(material, indent=2, sort_keys=True) + "\n")
    refresh_manifest_asset(out_dir, name)
    refresh_manifest_asset(out_dir, INTEGRITY_NAME)


def refresh_manifest_asset(out_dir: Path, name: str) -> None:
    path = out_dir / name
    updates = {"sha256": sha256(path), "size_bytes": path.stat().st_size}

    manifest_path = out_dir / UPLOAD_MANIFEST_NAME
    manifest = json.loads(manifest_path.read_text())
    for asset in manifest["public_assets"]:
        if asset["name"] == name:
            asset.update(updates)
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def run_verify(
    tarball: Path,
    *,
    check: bool = True,
    verify_sidecars: bool = False,
    downloaded_public_assets: bool = False,
    verify_integrity: bool = False,
    release_tag: str = "v0.2.9",
    gh_bin: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(VERIFY),
        str(tarball),
        "--repo-root",
        str(REPO_ROOT),
        "--release-tag",
        release_tag,
    ]
    if verify_sidecars:
        cmd.append("--verify-sidecars")
    if downloaded_public_assets:
        cmd.append("--downloaded-public-assets")
    if verify_integrity:
        if gh_bin is None:
            gh_bin = fake_gh_path(tarball.parent.parent)
        cmd.extend(
            [
                "--verify-integrity",
                "--commit-sha",
                INTEGRITY_COMMIT_SHA,
                "--trust-policy",
                str(trust_policy_path(tarball.parent)),
                "--attestation-bundle",
                str(tarball.parent / INTEGRITY_ATTESTATION_BUNDLE_NAME),
                "--attestation-metadata",
                str(tarball.parent / INTEGRITY_ATTESTATION_METADATA_NAME),
                "--verification-time",
                INTEGRITY_VERIFICATION_TIME,
                "--gh-bin",
                str(gh_bin),
                "--rust-toolchain",
                INTEGRITY_RUST_TOOLCHAIN,
            ]
        )
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def run_release_upload_manifest(
    out_dir: Path,
    *args: str,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(UPLOAD_MANIFEST),
        "--dist-dir",
        str(out_dir),
        "--release-tag",
        "v0.2.9",
        *args,
    ]
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def run_release_publish_receipt(
    out_dir: Path,
    *args: str,
    check: bool = True,
    actor: str = "release-bot",
    github_ref: str = "refs/tags/v0.2.9",
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(PUBLISH_RECEIPT),
        "--dist-dir",
        str(out_dir),
        "--release-tag",
        "v0.2.9",
        "--commit-sha",
        INTEGRITY_COMMIT_SHA,
        "--workflow-run-id",
        "12345",
        "--workflow-run-attempt",
        "1",
        "--actor",
        actor,
        "--repository",
        "moradology/m80",
        "--github-ref",
        github_ref,
        "--generated-at",
        "2026-05-21T00:00:00Z",
        *args,
    ]
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def run_release_evidence_bundle(
    out_dir: Path,
    *args: str,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(EVIDENCE_BUNDLE),
        "--dist-dir",
        str(out_dir),
        "--release-tag",
        "v0.2.9",
        "--commit-sha",
        INTEGRITY_COMMIT_SHA,
        "--workflow-run-id",
        "12345",
        "--m80-version",
        "v0.2.9",
        "--resolved-install-tag",
        "v0.2.9",
        "--generated-at",
        "2026-05-21T00:00:00Z",
        *args,
    ]
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def run_remote_asset_inventory(
    redownload_dir: Path,
    manifest_dir: Path,
    metadata: Path,
    *args: str,
    assets_metadata: Path | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(REMOTE_INVENTORY),
        "--redownload-dir",
        str(redownload_dir),
        "--manifest",
        str(manifest_dir / UPLOAD_MANIFEST_NAME),
        "--release-metadata",
        str(metadata),
        "--release-tag",
        "v0.2.9",
        "--generated-at",
        "2026-05-21T00:00:00Z",
    ]
    if assets_metadata is not None:
        cmd.extend(["--release-assets-metadata", str(assets_metadata)])
    cmd.extend(args)
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
        "v0.2.9",
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
        "v0.2.9",
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


def load_release_integrity_contract() -> dict:
    with RELEASE_INTEGRITY_CONTRACT.open() as f:
        return json.load(f)


def sha256(path: Path) -> str:
    import hashlib

    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data: bytes) -> str:
    import hashlib

    return hashlib.sha256(data).hexdigest()


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
