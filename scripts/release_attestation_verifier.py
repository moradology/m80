"""Shared preflight for the release attestation verifier toolchain."""

from __future__ import annotations

import subprocess


REQUIRED_GH_ATTESTATION_FLAGS = (
    "--repo",
    "--bundle",
    "--signer-workflow",
    "--cert-oidc-issuer",
    "--source-ref",
    "--source-digest",
    "--deny-self-hosted-runners",
    "--format",
)
WHY = (
    "signed m80 release verification requires `gh attestation verify` before "
    "trusting release assets"
)
LINUX_REMEDIATION = (
    "Install or upgrade GitHub CLI with attestation support on Linux: "
    "https://cli.github.com/packages"
)


def preflight_gh_attestation_verifier(gh_bin: str) -> None:
    version = run_probe_or_missing(gh_bin, ["--version"], why=WHY)
    if version.returncode != 0:
        raise SystemExit(
            "release attestation verifier unsupported: "
            f"{gh_bin}; {WHY}; observed version output "
            f"{format_probe(version)}; {LINUX_REMEDIATION}"
        )

    help_result = run_probe_or_missing(
        gh_bin,
        ["attestation", "verify", "--help"],
        why="`gh attestation verify --help` must be available before trusting release assets",
        version=version,
    )
    if help_result.returncode != 0:
        raise SystemExit(
            "release attestation verifier unsupported: "
            f"{gh_bin}; `gh attestation verify --help` failed; "
            f"observed version output {format_probe(version)}; "
            f"observed help output {format_probe(help_result)}; "
            f"{WHY}; {LINUX_REMEDIATION}"
        )

    help_text = combined_output(help_result)
    missing = [flag for flag in REQUIRED_GH_ATTESTATION_FLAGS if flag not in help_text]
    if missing:
        raise SystemExit(
            "release attestation verifier unsupported: "
            f"{gh_bin}; `gh attestation verify --help` is missing required flag(s): "
            f"{', '.join(missing)}; observed version output {format_probe(version)}; "
            f"observed help output {format_probe(help_result)}; {WHY}; {LINUX_REMEDIATION}"
        )


def run_probe_or_missing(
    gh_bin: str,
    args: list[str],
    *,
    why: str,
    version: subprocess.CompletedProcess[str] | None = None,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            [gh_bin, *args],
            check=False,
            text=True,
            capture_output=True,
        )
    except OSError as exc:
        observed = ""
        if version is not None:
            observed = f"; observed version output {format_probe(version)}"
        raise SystemExit(
            f"release attestation verifier missing: {gh_bin}; {why}{observed}; "
            f"{LINUX_REMEDIATION}; spawn error: {exc}"
        ) from exc


def combined_output(result: subprocess.CompletedProcess[str]) -> str:
    return f"{result.stdout}{result.stderr}"


def format_probe(result: subprocess.CompletedProcess[str]) -> str:
    text = combined_output(result).strip()
    if not text:
        text = "<no output>"
    else:
        text = text.replace("\n", "\\n")
    return f"exit={result.returncode} {text}"
