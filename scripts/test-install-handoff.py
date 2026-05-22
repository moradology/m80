#!/usr/bin/env python3
"""Tests for install.sh verification before privileged handoff."""

from __future__ import annotations

import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY_HANDOFF = REPO_ROOT / "scripts" / "verify-install-handoff.py"
INSTALL_NAME = "install.sh"
CHECKSUM_NAME = f"{INSTALL_NAME}.sha256"
INTEGRITY_NAME = "m80-release-integrity.json"
ATTESTATION_BUNDLE_NAME = "m80-release-integrity.attestation.jsonl"
ATTESTATION_METADATA_NAME = "m80-release-attestation.json"
TRUST_POLICY_NAME = "m80-release-trust-policy.json"
RELEASE_TAG = "v0.0.0"
COMMIT_SHA = "0123456789abcdef0123456789abcdef01234567"
VERIFICATION_TIME = "2026-05-20T00:00:00Z"
SIGNER_IDENTITY = "moradology/m80/.github/workflows/release-artifacts.yml"
SIGNER_ISSUER = "https://token.actions.githubusercontent.com"
KEYSET_ID = "github-actions-oidc:m80-release-v1"


class InstallHandoffTest(unittest.TestCase):
    def test_verifies_install_sh_before_printing_sudo_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = write_handoff_fixture(root)

            result = run_handoff(dist)

        self.assertIn(f"verified install handoff: release_tag={RELEASE_TAG} commit={COMMIT_SHA}", result.stdout)
        self.assertIn(f"install_sh_sha256={sha256_text('#!/bin/sh\necho install\n')}", result.stdout)
        self.assertIn(
            "verified_assets=install.sh,install.sh.sha256,m80-release-integrity.json,"
            "m80-release-integrity.attestation.jsonl,m80-release-attestation.json",
            result.stdout,
        )
        self.assertIn("sudo sh ", result.stdout)

    def test_json_output_names_verified_assets_and_local_sudo_command(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = write_handoff_fixture(root)
            expected_install_sha = sha256(dist / INSTALL_NAME)

            result = run_handoff(dist, json_output=True)

        payload = json.loads(result.stdout)
        self.assertTrue(payload["verified_install_handoff"])
        self.assertEqual(payload["release_tag"], RELEASE_TAG)
        self.assertEqual(payload["install_sh_sha256"], expected_install_sha)
        self.assertEqual(payload["verified_assets"][0:2], [INSTALL_NAME, CHECKSUM_NAME])
        self.assertEqual(payload["sudo_command"], f"sudo sh {dist / INSTALL_NAME}")

    def test_rejects_tampered_install_before_sudo_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = write_handoff_fixture(root)
            with (dist / INSTALL_NAME).open("a") as f:
                f.write("# tampered\n")

            result = run_handoff(dist, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("install.sh sha256 mismatch", result.stderr)
        self.assertNotIn("sudo sh", result.stdout)

    def test_rejects_wrong_checksum_before_sudo_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = write_handoff_fixture(root)
            (dist / CHECKSUM_NAME).write_text(f"{'0' * 64}  {INSTALL_NAME}\n")

            result = run_handoff(dist, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("install.sh sha256 mismatch", result.stderr)
        self.assertNotIn("sudo sh", result.stdout)

    def test_rejects_missing_signature_material_before_sudo_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = write_handoff_fixture(root)
            (dist / ATTESTATION_BUNDLE_NAME).unlink()

            result = run_handoff(dist, check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release attestation bundle missing", result.stderr)
        self.assertNotIn("sudo sh", result.stdout)

    def test_rejects_wrong_tag_before_sudo_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dist = write_handoff_fixture(root)

            result = run_handoff(dist, release_tag="v9.9.9", check=False)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release integrity release_tag mismatch", result.stderr)
        self.assertNotIn("sudo sh", result.stdout)

    def test_release_workflow_runs_handoff_verifier_on_published_assets(self) -> None:
        workflow = (REPO_ROOT / ".github" / "workflows" / "release-artifacts.yml").read_text()

        self.assertIn("scripts/verify-install-handoff.py", workflow)
        self.assertIn("m80-release-integrity.attestation.jsonl", workflow)


def run_handoff(
    dist: Path,
    *,
    release_tag: str = RELEASE_TAG,
    json_output: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    cmd = [
        "python3",
        str(VERIFY_HANDOFF),
        str(dist),
        "--release-tag",
        release_tag,
        "--trust-policy",
        str(dist / TRUST_POLICY_NAME),
        "--verification-time",
        VERIFICATION_TIME,
        "--gh-bin",
        str(fake_gh_path(dist.parent)),
    ]
    if json_output:
        cmd.append("--json")
    return subprocess.run(cmd, check=check, text=True, capture_output=True)


def write_handoff_fixture(root: Path) -> Path:
    dist = root / "dist"
    dist.mkdir()
    install = dist / INSTALL_NAME
    install.write_text("#!/bin/sh\necho install\n")
    install.chmod(0o755)
    write_sha256_sidecar(dist / CHECKSUM_NAME, install, INSTALL_NAME)
    material = write_integrity_material(dist)
    write_trust_policy(dist)
    write_attestation_bundle(dist, material)
    write_attestation_metadata(dist, material)
    write_fake_gh(root)
    return dist


def write_integrity_material(dist: Path) -> Path:
    subjects = []
    for name, kind in [(INSTALL_NAME, "installer"), (CHECKSUM_NAME, "checksum-sidecar")]:
        asset = dist / name
        subjects.append(
            {
                "name": name,
                "kind": kind,
                "sha256": sha256(asset),
                "size_bytes": asset.stat().st_size,
            }
        )
    payload = {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "release_tag": RELEASE_TAG,
        "commit_sha": COMMIT_SHA,
        "target": "linux-x86_64",
        "rust_toolchain": "1.82",
        "m80_package_version": "0.0.0",
        "bundle_metadata_name": "m80-linux-x86_64.bundle.json",
        "bundle_metadata_sha256": "a" * 64,
        "subjects": subjects,
    }
    path = dist / INTEGRITY_NAME
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return path


def write_trust_policy(dist: Path) -> None:
    payload = {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "keyset_id": KEYSET_ID,
        "valid_from": "2026-01-01T00:00:00Z",
        "valid_until": "2027-01-01T00:00:00Z",
        "allowed_signers": [{"identity": SIGNER_IDENTITY, "issuer": SIGNER_ISSUER}],
        "rotation": {"mode": "hard-fail-expired", "overlap_days": 14, "next_keyset_id": None},
    }
    (dist / TRUST_POLICY_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def write_attestation_bundle(dist: Path, material: Path) -> None:
    statement = {
        "_type": "https://in-toto.io/Statement/v1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": [
            {
                "name": material.name,
                "digest": {"sha256": sha256(material)},
            }
        ],
        "predicate": {
            "buildDefinition": {
                "buildType": "https://actions.github.io/buildtypes/workflow/v1",
                "externalParameters": {
                    "workflow": {
                        "repository": "https://github.com/moradology/m80",
                        "path": ".github/workflows/release-artifacts.yml",
                        "ref": f"refs/tags/{RELEASE_TAG}",
                    }
                },
                "internalParameters": {
                    "github": {"runner_environment": "github-hosted"}
                },
                "resolvedDependencies": [
                    {
                        "uri": f"git+https://github.com/moradology/m80@refs/tags/{RELEASE_TAG}",
                        "digest": {"gitCommit": COMMIT_SHA},
                    }
                ],
            },
            "runDetails": {
                "builder": {
                    "id": f"https://github.com/moradology/m80/.github/workflows/release-artifacts.yml@refs/tags/{RELEASE_TAG}"
                }
            },
        },
    }
    payload = {
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {
            "certificate": {"rawBytes": "fixture-cert"},
            "tlogEntries": [{"logIndex": "1"}],
        },
        "dsseEnvelope": {
            "payloadType": "application/vnd.in-toto+json",
            "payload": base64.b64encode(json.dumps(statement, sort_keys=True).encode()).decode(),
            "signatures": [{"sig": "fixture"}],
        },
    }
    (dist / ATTESTATION_BUNDLE_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def write_attestation_metadata(dist: Path, material: Path) -> None:
    payload = {
        "schema_version": 1,
        "mechanism": "github-artifact-attestation",
        "repository": "moradology/m80",
        "release_tag": RELEASE_TAG,
        "predicate_sha256": sha256(material),
        "signer_identity": SIGNER_IDENTITY,
        "issuer": SIGNER_ISSUER,
        "keyset_id": KEYSET_ID,
        "certificate_not_before": "2026-01-01T00:00:00Z",
        "certificate_not_after": "2027-01-01T00:00:00Z",
    }
    (dist / ATTESTATION_METADATA_NAME).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def write_fake_gh(root: Path) -> None:
    script = f"""#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import sys

args = sys.argv[1:]
if args == ["--version"]:
    print("gh version 9.9.9")
    sys.exit(0)
if args == ["attestation", "verify", "--help"]:
    print("--repo --bundle --signer-workflow --cert-oidc-issuer --source-ref --source-digest --deny-self-hosted-runners --format")
    sys.exit(0)
if len(args) < 3 or args[:2] != ["attestation", "verify"]:
    print("unexpected gh command", file=sys.stderr)
    sys.exit(1)
material = Path(args[2])
flags = {{}}
i = 3
while i < len(args):
    flag = args[i]
    if flag == "--deny-self-hosted-runners":
        flags[flag] = True
        i += 1
    else:
        flags[flag] = args[i + 1]
        i += 2
expected = {{
    "--repo": "moradology/m80",
    "--bundle": str(material.parent / "{ATTESTATION_BUNDLE_NAME}"),
    "--signer-workflow": "{SIGNER_IDENTITY}",
    "--cert-oidc-issuer": "{SIGNER_ISSUER}",
    "--source-ref": "refs/tags/{RELEASE_TAG}",
    "--source-digest": "{COMMIT_SHA}",
    "--format": "json",
}}
for flag, value in expected.items():
    if flags.get(flag) != value:
        print(f"{{flag}} mismatch: {{flags.get(flag)}}", file=sys.stderr)
        sys.exit(1)
if flags.get("--deny-self-hosted-runners") is not True:
    print("--deny-self-hosted-runners missing", file=sys.stderr)
    sys.exit(1)
bundle = json.loads(Path(flags["--bundle"]).read_text())
if not bundle.get("valid", False):
    print("cryptographic attestation invalid", file=sys.stderr)
    sys.exit(1)
if bundle.get("artifact") != str(material):
    print("artifact mismatch", file=sys.stderr)
    sys.exit(1)
digest = hashlib.sha256(material.read_bytes()).hexdigest()
print(json.dumps([{{"verificationResult": {{"statement": {{"subject": [{{"name": str(material), "digest": {{"sha256": digest}}}}]}}}}}}]))
"""
    path = fake_gh_path(root)
    path.write_text(script)
    path.chmod(0o755)


def fake_gh_path(root: Path) -> Path:
    return root / "fake-gh"


def write_sha256_sidecar(path: Path, asset: Path, asset_name: str) -> None:
    path.write_text(f"{sha256(asset)}  {asset_name}\n")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


if __name__ == "__main__":
    unittest.main()
