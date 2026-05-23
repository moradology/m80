use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::FcError;

use super::process::{command_label, print_command_output, run_output_capture};

pub(super) const REQUIRED_ARTIFACTS: &[&str] = &[
    "vmlinux",
    "output.ext4",
    "output.ext4.manifest.json",
    "output.ext4.build-receipt.json",
    "m80-guestd",
];

const FORBIDDEN_ARTIFACTS: &[&str] = &[
    "host-binaries.manifest.json",
    "firecracker",
    "jailer",
    "firecracker-seccomp-filter.bin",
    "firecracker-seccomp-filter.json",
];

const FORBIDDEN_HOST_PREREQ_REASON: &str = concat!(
    "m80 v0.x release artifacts must not bundle official Firecracker, official jailer, ",
    "or Firecracker seccomp filter payloads; m80 owns m80 binaries/helpers and guest ",
    "artifacts, while Firecracker/jailer/seccomp are operator-provided host prerequisites",
);

pub(super) fn artifact_checksum_url(artifact_url: &str) -> String {
    format!("{artifact_url}.sha256")
}

pub(super) fn verify_tarball_checksum(
    tarball: &Path,
    checksum_file: &Path,
    json_output: bool,
) -> Result<(), FcError> {
    let expected = read_expected_sha256(checksum_file)?;
    let output = run_output_capture(
        Command::new("sha256sum").arg(tarball),
        "compute artifact tarball checksum",
    )?;
    if !json_output {
        print_command_output(&output.stderr);
    }
    if !output.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let output_text = if combined.is_empty() {
            String::new()
        } else {
            format!(": {combined}")
        };
        return Err(FcError::CommandFailed {
            command: command_label("compute artifact tarball checksum"),
            status: output.status,
            output: output_text,
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let actual = stdout.split_whitespace().next().ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "artifact_url.sha256",
            reason: "sha256sum produced no digest for artifact tarball".to_owned(),
        })
    })?;
    if actual != expected {
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url.sha256",
                reason: format!(
                    "artifact tarball sha256 mismatch: expected {expected}, got {actual}"
                ),
            },
        ));
    }
    Ok(())
}

fn read_expected_sha256(checksum_file: &Path) -> Result<String, FcError> {
    let text = fs::read_to_string(checksum_file).map_err(|e| FcError::PathIo {
        path: checksum_file.to_path_buf(),
        source: e,
    })?;
    let expected = text.split_whitespace().next().ok_or_else(|| {
        FcError::Config(m80_firecracker::ConfigError::InvalidValue {
            field: "artifact_url.sha256",
            reason: format!("checksum file {} is empty", checksum_file.display()),
        })
    })?;
    if expected.len() != 64 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(FcError::Config(
            m80_firecracker::ConfigError::InvalidValue {
                field: "artifact_url.sha256",
                reason: format!(
                    "checksum file {} must start with a 64-hex sha256 digest",
                    checksum_file.display()
                ),
            },
        ));
    }
    Ok(expected.to_ascii_lowercase())
}

pub(super) fn find_forbidden_artifact(root: &Path) -> Result<Option<PathBuf>, FcError> {
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).map_err(|source| FcError::PathIo {
            path: dir.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| FcError::PathIo {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let file_name = entry.file_name();
            if FORBIDDEN_ARTIFACTS
                .iter()
                .any(|forbidden| file_name == OsStr::new(forbidden))
            {
                return Ok(Some(path));
            }
            let file_type = entry.file_type().map_err(|source| FcError::PathIo {
                path: path.clone(),
                source,
            })?;
            if file_type.is_dir() {
                dirs.push(path);
            }
        }
    }
    Ok(None)
}

pub(super) fn forbidden_artifact_reason(name: &str) -> String {
    if name == "host-binaries.manifest.json" {
        format!(
            "artifact tarball must not contain {name}; generate it after final host install paths are known"
        )
    } else {
        format!("artifact tarball must not contain {name}; {FORBIDDEN_HOST_PREREQ_REASON}")
    }
}
