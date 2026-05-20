#!/usr/bin/env python3
"""Run the pinned actionlint binary after checksum-verifying its archive."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request


@dataclass(frozen=True)
class ActionlintPin:
    version: str
    target: str
    archive_name: str
    sha256: str
    url: str


PIN = ActionlintPin(
    version="1.7.12",
    target="linux_amd64",
    archive_name="actionlint_1.7.12_linux_amd64.tar.gz",
    sha256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
    url="https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz",
)


class ActionlintRunnerError(Exception):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--workflow-dir",
        type=Path,
        default=Path(".github/workflows"),
        help="workflow directory to lint",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=default_cache_dir(),
        help="cache directory for the verified actionlint archive and binary",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        return run_actionlint(args.workflow_dir, args.cache_dir, PIN)
    except ActionlintRunnerError as err:
        print(f"actionlint runner failed: {err}", file=sys.stderr)
        return 1


def default_cache_dir() -> Path:
    if env := os.environ.get("M80_ACTIONLINT_CACHE_DIR"):
        return Path(env)
    base = os.environ.get("XDG_CACHE_HOME")
    if base:
        return Path(base) / "m80" / "actionlint"
    return Path.home() / ".cache" / "m80" / "actionlint"


def run_actionlint(
    workflow_dir: Path,
    cache_dir: Path,
    pin: ActionlintPin,
    *,
    env: dict[str, str] | None = None,
    host_target: str | None = None,
) -> int:
    target = host_target if host_target is not None else supported_target()
    if target != pin.target:
        raise ActionlintRunnerError(f"pinned actionlint target {pin.target} cannot run on host target {target}")
    binary = ensure_actionlint(cache_dir, pin)
    workflow_files = sorted(workflow_dir.glob("*.yml")) + sorted(workflow_dir.glob("*.yaml"))
    if not workflow_files:
        raise ActionlintRunnerError(f"{workflow_dir}: no workflow files found")
    command = [str(binary), "-no-color", *[str(path) for path in workflow_files]]
    return subprocess.run(command, env=env, check=False).returncode


def supported_target(
    *,
    system: str | None = None,
    machine: str | None = None,
) -> str:
    system = system if system is not None else platform.system()
    machine = machine if machine is not None else platform.machine()
    normalized_system = system.lower()
    normalized_machine = machine.lower()
    if normalized_system == "linux" and normalized_machine in {"x86_64", "amd64"}:
        return "linux_amd64"
    raise ActionlintRunnerError(
        f"unsupported host platform for pinned actionlint: system={system!r} machine={machine!r}; "
        "only Linux x86_64 is pinned"
    )


def ensure_actionlint(cache_dir: Path, pin: ActionlintPin) -> Path:
    release_dir = cache_dir / f"v{pin.version}" / pin.target
    archive = release_dir / pin.archive_name
    binary = release_dir / "actionlint"
    release_dir.mkdir(parents=True, exist_ok=True)
    ensure_archive(archive, pin)
    extract_actionlint_binary(archive, binary)
    return binary


def ensure_archive(path: Path, pin: ActionlintPin) -> None:
    if path.exists():
        verify_archive_sha256(path, pin)
        return
    download_archive(path, pin)
    verify_archive_sha256(path, pin)


def download_archive(path: Path, pin: ActionlintPin) -> None:
    fd, tmp_name = tempfile.mkstemp(prefix=f".{pin.archive_name}.", suffix=".tmp", dir=path.parent)
    tmp = Path(tmp_name)
    try:
        with os.fdopen(fd, "wb") as out:
            try:
                with urllib.request.urlopen(pin.url, timeout=60) as response:
                    while chunk := response.read(1024 * 1024):
                        out.write(chunk)
            except (OSError, urllib.error.URLError) as err:
                raise ActionlintRunnerError(
                    f"failed to download pinned actionlint {pin.version} from {pin.url}: {err}"
                ) from err
        verify_archive_sha256(tmp, pin)
        tmp.replace(path)
    except Exception:
        tmp.unlink(missing_ok=True)
        raise


def verify_archive_sha256(path: Path, pin: ActionlintPin) -> None:
    actual = sha256_file(path)
    if actual != pin.sha256:
        raise ActionlintRunnerError(
            f"actionlint archive sha256 mismatch for {path}: expected {pin.sha256}, got {actual}; "
            "refusing to run unverified actionlint"
        )


def extract_actionlint_binary(archive: Path, binary: Path) -> None:
    try:
        with tarfile.open(archive, "r:gz") as tar:
            member = tar.getmember("actionlint")
            if not member.isfile():
                raise ActionlintRunnerError(f"actionlint archive {archive} did not contain a file named actionlint")
            source = tar.extractfile(member)
            if source is None:
                raise ActionlintRunnerError(f"actionlint archive {archive} did not expose actionlint bytes")
            tmp = binary.with_name(f".{binary.name}.tmp")
            with source, tmp.open("wb") as out:
                while chunk := source.read(1024 * 1024):
                    out.write(chunk)
            tmp.chmod(
                stat.S_IRUSR
                | stat.S_IWUSR
                | stat.S_IXUSR
                | stat.S_IRGRP
                | stat.S_IXGRP
                | stat.S_IROTH
                | stat.S_IXOTH
            )
            tmp.replace(binary)
    except KeyError as err:
        raise ActionlintRunnerError(f"actionlint archive {archive} did not contain a file named actionlint") from err
    except tarfile.TarError as err:
        raise ActionlintRunnerError(f"failed to read verified actionlint archive {archive}: {err}") from err


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        while chunk := file.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


if __name__ == "__main__":
    sys.exit(main())
