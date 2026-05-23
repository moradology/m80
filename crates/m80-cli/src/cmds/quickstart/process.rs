use std::process::{Command, Stdio};

use m80_firecracker::FcError;

pub(super) fn run_status(cmd: &mut Command, label: &str) -> Result<(), FcError> {
    let status = cmd.status().map_err(|source| FcError::CommandSpawnFailed {
        command: command_label(label),
        source,
    })?;
    if !status.success() {
        return Err(FcError::CommandFailed {
            command: command_label(label),
            status,
            output: String::new(),
        });
    }
    Ok(())
}

pub(super) fn run_output(cmd: &mut Command, label: &str, json_output: bool) -> Result<(), FcError> {
    let command_output = run_output_capture(cmd, label)?;
    if !json_output {
        print_command_output(&command_output.stdout);
        print_command_output(&command_output.stderr);
    }
    if !command_output.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&command_output.stdout),
            String::from_utf8_lossy(&command_output.stderr)
        );
        let output = if combined.is_empty() {
            String::new()
        } else {
            format!(": {combined}")
        };
        return Err(FcError::CommandFailed {
            command: command_label(label),
            status: command_output.status,
            output,
        });
    }
    Ok(())
}

pub(super) fn run_output_capture(
    cmd: &mut Command,
    label: &str,
) -> Result<std::process::Output, FcError> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| FcError::CommandSpawnFailed {
            command: command_label(label),
            source,
        })
}

pub(super) fn command_label(label: &str) -> &'static str {
    match label {
        "curl artifact tarball" => "curl artifact tarball",
        "curl artifact checksum" => "curl artifact checksum",
        "compute artifact tarball checksum" => "compute artifact tarball checksum",
        "compute file checksum" => "compute file checksum",
        "extract artifact tarball" => "extract artifact tarball",
        "verify artifact checksums" => "verify artifact checksums",
        _ => "quickstart helper",
    }
}

pub(super) fn print_command_output(bytes: &[u8]) {
    if !bytes.is_empty() {
        eprint!("{}", String::from_utf8_lossy(bytes));
    }
}
