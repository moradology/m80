#!/usr/bin/env python3
"""Exercise release asset-index JSON errors through a release-identity m80 binary."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
COMMIT = "0123456789abcdef0123456789abcdef01234567"


def main() -> None:
    version = workspace_version()
    release_tag = f"v{version}"
    tmp_parent = Path("/tank/tmp") if Path("/tank/tmp").is_dir() else None
    with tempfile.TemporaryDirectory(prefix="m80-release-cli-", dir=tmp_parent) as tmp:
        root = Path(tmp)
        fixture_dir = root / "fixture-index"
        fixture_dir.mkdir()
        index_path = fixture_dir / "m80-release-assets.json"
        binary = build_release_binary(root, release_tag, index_path)
        arch = rust_arch()

        cases = [
            (
                "unsupported_host_tuple",
                release_tag,
                index_json(
                    release_tag,
                    [
                        asset_json(
                            os_name="linux",
                            arch="fixture_arch",
                            image_kind="minimal",
                            release_tag=release_tag,
                            m80_version=release_tag,
                        )
                    ],
                ),
            ),
            (
                "missing_image_kind",
                release_tag,
                index_json(
                    release_tag,
                    [
                        asset_json(
                            os_name="linux",
                            arch=arch,
                            image_kind="ubuntu",
                            release_tag=release_tag,
                            m80_version=release_tag,
                        )
                    ],
                ),
            ),
            (
                "stale_asset_index",
                release_tag,
                index_json(
                    release_tag,
                    [
                        asset_json(
                            os_name="linux",
                            arch=arch,
                            image_kind="minimal",
                            release_tag=release_tag,
                            m80_version="v9.9.9",
                        )
                    ],
                ),
            ),
            (
                "duplicate_default_bundle",
                release_tag,
                index_json(
                    release_tag,
                    [
                        asset_json(
                            os_name="linux",
                            arch=arch,
                            image_kind="minimal",
                            release_tag=release_tag,
                            m80_version=release_tag,
                        ),
                        asset_json(
                            os_name="linux",
                            arch=arch,
                            image_kind="minimal",
                            release_tag=release_tag,
                            m80_version=release_tag,
                            name_suffix="-duplicate",
                        ),
                    ],
                ),
            ),
            (
                "binary_tag_mismatch",
                "v9.9.9",
                index_json(release_tag, []),
            ),
        ]

        for code, requested_tag, index_text in cases:
            write_index(index_path, index_text)
            install_root = root / f"install-{code}"
            result = subprocess.run(
                [
                    str(binary),
                    "--json",
                    "install",
                    "--release-tag",
                    requested_tag,
                    "--install-root",
                    str(install_root),
                    "--dry-run",
                ],
                cwd=ROOT,
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            assert result.returncode == 6, result
            assert result.stdout == "", result.stdout
            payload = json.loads(result.stderr)
            data = payload["data"]
            assert payload["version"] == 1, payload
            assert data["variant"] == "ReleaseAssetIndex", payload
            assert data["exit_code"] == 6, payload
            assert data["code"] == code, payload
            assert data["requested_os"] == "linux", payload
            assert data["requested_arch"] == arch, payload
            assert data["requested_image_kind"] == "minimal", payload
            assert data["requested_release_tag"] == requested_tag, payload
            assert data["requested_m80_version"] == release_tag, payload
            assert not install_root.exists(), install_root
            if code != "duplicate_default_bundle":
                assert data.get("repair_command"), payload

    print("release identity CLI fixture asset-index errors: ok")


def workspace_version() -> str:
    text = (ROOT / "Cargo.toml").read_text()
    match = re.search(r"(?m)^version\s*=\s*\"([^\"]+)\"", text)
    if match is None:
        raise RuntimeError("workspace version not found")
    return match.group(1)


def build_release_binary(root: Path, release_tag: str, index_path: Path) -> Path:
    target_dir = root / "target"
    env = os.environ.copy()
    env.update(
        {
            "CARGO_TARGET_DIR": str(target_dir),
            "M80_RELEASE_TAG": release_tag,
            "M80_RELEASE_COMMIT": COMMIT,
            "M80_RELEASE_TARGET_TRIPLE": rust_target_triple(),
            "M80_INTERNAL_RELEASE_FIXTURE_ASSET_INDEX_URL": f"file://{index_path}",
        }
    )
    subprocess.run(
        ["cargo", "build", "-p", "m80-cli", "--bin", "m80"],
        cwd=ROOT,
        env=env,
        check=True,
    )
    binary = target_dir / "debug" / "m80"
    if not binary.exists():
        raise RuntimeError(f"release fixture binary missing: {binary}")
    return binary


def rust_target_triple() -> str:
    output = subprocess.check_output(["rustc", "-vV"], cwd=ROOT, text=True)
    for line in output.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ")
    raise RuntimeError("rustc host triple not found")


def rust_arch() -> str:
    machine = platform.machine().lower()
    if machine in {"x86_64", "amd64"}:
        return "x86_64"
    if machine in {"aarch64", "arm64"}:
        return "aarch64"
    return machine


def write_index(path: Path, text: str) -> None:
    path.write_text(text)
    digest = hashlib.sha256(text.encode()).hexdigest()
    path.with_name(path.name + ".sha256").write_text(f"{digest}  {path.name}\n")


def index_json(release_tag: str, assets: list[str]) -> str:
    return json.dumps(
        {
            "schema_version": 1,
            "release_tag": release_tag,
            "assets": [json.loads(asset) for asset in assets],
        },
        sort_keys=True,
    )


def asset_json(
    *,
    os_name: str,
    arch: str,
    image_kind: str,
    release_tag: str,
    m80_version: str,
    name_suffix: str = "",
) -> str:
    target = f"{os_name}-{arch}"
    name = f"m80-{target}{name_suffix}.tar.gz"
    return json.dumps(
        {
            "name": name,
            "url": f"https://github.com/moradology/m80/releases/download/{release_tag}/{name}",
            "sha256": "a" * 64,
            "size_bytes": 42,
            "metadata_name": f"m80-{target}{name_suffix}.bundle.json",
            "metadata_sha256": "b" * 64,
            "checksum_name": f"{name}.sha256",
            "signature_name": "m80.sig",
            "attestation_name": "m80.intoto.jsonl",
            "target": target,
            "os": os_name,
            "arch": arch,
            "image_kind": image_kind,
            "release_tag": release_tag,
            "m80_version": m80_version,
            "guest_protocol_version": 1,
            "manifest_schema_version": 1,
            "expected_firecracker_version": "v1.15.1",
        },
        sort_keys=True,
    )


if __name__ == "__main__":
    main()
