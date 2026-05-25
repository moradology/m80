use std::io;
use std::process::Command;

use m80_firecracker::FcError;

pub(super) trait HostCommands {
    fn output(&mut self, program: &str, args: &[&str]) -> Result<HostCommandOutput, FcError>;

    fn run(&mut self, program: &str, args: &[&str]) -> Result<(), FcError> {
        let output = self.output(program, args)?;
        if output.status_success {
            Ok(())
        } else {
            Err(command_failed(program, &output.stderr))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HostCommandOutput {
    pub(super) status_success: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) struct RealHostCommands;

impl HostCommands for RealHostCommands {
    fn output(&mut self, program: &str, args: &[&str]) -> Result<HostCommandOutput, FcError> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|source| FcError::HostIo {
                operation: "m80 net cleanup command",
                source,
            })?;
        Ok(HostCommandOutput {
            status_success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub(super) fn command_failed(program: &str, stderr: &str) -> FcError {
    let detail = if stderr.trim().is_empty() {
        format!("{program} exited non-zero")
    } else {
        format!("{program}: {}", stderr.trim())
    };
    FcError::HostIo {
        operation: "m80 net cleanup command",
        source: io::Error::new(io::ErrorKind::Other, detail),
    }
}
