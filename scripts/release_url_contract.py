"""Shared public GitHub release URL contract for m80 installers."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import re


REPO_ROOT = Path(__file__).resolve().parents[1]
CONTRACT_PATH = REPO_ROOT / "docs" / "behaviors" / "release" / "public-release-root.env"
TOKEN_RE = re.compile(r"^[A-Za-z0-9_.-]+$")
VERIFIED_INSTALL_HANDOFF_ASSETS = (
    "install.sh",
    "install.sh.sha256",
    "SHA256SUMS",
    "m80-release-integrity.json",
    "m80-release-integrity.attestation.jsonl",
    "m80-release-attestation.json",
)


@dataclass(frozen=True)
class PublicReleaseRoot:
    owner: str
    repo: str

    @property
    def repository(self) -> str:
        return f"{self.owner}/{self.repo}"

    @property
    def latest_base_url(self) -> str:
        return f"https://github.com/{self.repository}/releases/latest/download"

    def download_base_url(self, release_tag: str) -> str:
        return f"https://github.com/{self.repository}/releases/download/{release_tag}"

    def asset_url(self, release_tag: str, asset_name: str) -> str:
        return f"{self.download_base_url(release_tag)}/{asset_name}"

    @property
    def latest_install_url(self) -> str:
        return f"{self.latest_base_url}/install.sh"

    def pinned_install_url(self, release_tag: str) -> str:
        return self.asset_url(release_tag, "install.sh")


def public_release_root(path: Path = CONTRACT_PATH) -> PublicReleaseRoot:
    values = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, sep, value = line.partition("=")
        if not sep:
            raise ValueError(f"{path}: malformed line: {line!r}")
        values[key] = value
    owner = required_token(values, "M80_PUBLIC_RELEASE_OWNER", path)
    repo = required_token(values, "M80_PUBLIC_RELEASE_REPO", path)
    return PublicReleaseRoot(owner=owner, repo=repo)


def required_token(values: dict[str, str], key: str, path: Path) -> str:
    value = values.get(key)
    if value is None:
        raise ValueError(f"{path}: missing {key}")
    if not TOKEN_RE.fullmatch(value):
        raise ValueError(f"{path}: {key} must be a GitHub owner/repo token")
    return value


def release_repository() -> str:
    return public_release_root().repository


def release_asset_url(release_tag: str, asset_name: str) -> str:
    return public_release_root().asset_url(release_tag, asset_name)


def latest_install_command() -> str:
    return f"curl -fsSL {public_release_root().latest_install_url} | sudo sh"


def pinned_install_command(release_tag: str = "<version>") -> str:
    return f"curl -fsSL {public_release_root().pinned_install_url(release_tag)} | sudo sh"


def verified_install_handoff_block(release_tag: str = "<version>") -> str:
    repo = public_release_root().repository
    assets = " ".join(VERIFIED_INSTALL_HANDOFF_ASSETS)
    return "\n".join(
        [
            f"tag={release_tag}",
            f"repo={repo}",
            'tmp="$(mktemp -d)"',
            'base="https://github.com/${repo}/releases/download/${tag}"',
            f"for asset in {assets}; do",
            '  curl -fsSLo "${tmp}/${asset}" "${base}/${asset}"',
            "done",
            'python3 scripts/verify-install-handoff.py "${tmp}" \\',
            '  --release-tag "${tag}" \\',
            "  --trust-policy docs/behaviors/release/m80-release-trust-policy.json \\",
            '  --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)"',
            'sudo sh "${tmp}/install.sh"',
        ]
    )
