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


class ReleaseBundleTest(unittest.TestCase):
    def test_packages_release_bundle_with_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            inputs = fixture_inputs(root, release_tag="v0.0.0")
            out_dir = root / "out"

            run_package(inputs, out_dir)

            tarball = out_dir / "m80-linux-x86_64.tar.gz"
            run_verify(tarball, verify_sidecars=True)
            self.assertTrue(tarball.is_file())
            self.assertTrue((out_dir / "m80-linux-x86_64.tar.gz.sha256").is_file())
            self.assertTrue((out_dir / "install.sh").is_file())
            self.assertTrue((out_dir / "install.sh.sha256").is_file())
            self.assertTrue((out_dir / "m80-linux-x86_64.bundle.json").is_file())
            self.assertTrue((out_dir / "m80-linux-x86_64.bundle.json.sha256").is_file())
            self.assertTrue((out_dir / "SHA256SUMS").is_file())
            self.assertEqual(file_mode(tarball), 0o644)
            self.assertEqual(file_mode(out_dir / "install.sh"), 0o755)
            self.assertEqual(file_mode(out_dir / "install.sh.sha256"), 0o644)
            self.assertEqual(file_mode(out_dir / "m80-linux-x86_64.bundle.json"), 0o644)
            self.assertEqual(file_mode(out_dir / "m80-linux-x86_64.bundle.json.sha256"), 0o644)
            self.assertEqual(file_mode(out_dir / "SHA256SUMS"), 0o644)
            with tarfile.open(tarball, "r:gz") as tar:
                names = set(tar.getnames())
                self.assertIn("bundle.json", names)
                self.assertIn("SHA256SUMS", names)
                self.assertIn("bin/m80", names)
                self.assertIn("artifacts/output.ext4.manifest.json", names)
                metadata = json.load(tar.extractfile("bundle.json"))  # type: ignore[arg-type]
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
                json.loads((out_dir / "m80-linux-x86_64.bundle.json").read_text()),
                metadata,
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
                "m80-linux-x86_64.bundle.json.sha256",
                "m80-linux-x86_64.bundle.json",
                "checksum sidecar mismatch",
            ),
            ("SHA256SUMS", "install.sh", "public SHA256SUMS hash mismatch for install.sh"),
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
    write_executable(inputs / "install.sh", "#!/bin/sh\nexit 0\n")
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
        "install": inputs / "install.sh",
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
    write_executable(path, f"#!/bin/sh\ncat <<'JSON'\n{json.dumps(payload)}\nJSON\n")


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
