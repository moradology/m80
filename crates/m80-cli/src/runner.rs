//! Testable CLI dispatch for the `m80` binary.
//!
//! The binary edge parses argv and calls [`run`]. Keeping dispatch here lets
//! unit tests exercise command routing without spawning the binary.

use crate::args::{Cli, Cmd, ConfigAction};
use crate::cmds;
use crate::cmds_walk;

/// Dispatch one parsed CLI invocation and return the process exit code.
pub fn run(cli: Cli) -> anyhow::Result<i32> {
    let json = cli.json;
    match cli.subcommand {
        Cmd::Run {
            profile,
            workspace,
            cwd,
            env,
            secret_env,
            stdin,
            egress,
            scratch_size,
            overlay_clone_mode,
            vcpu_count,
            mem_size_mib,
            writeback,
            tty,
            interactive,
            warm,
            argv,
        } => cmds::cmd_run(
            profile,
            workspace,
            cwd,
            env,
            secret_env,
            stdin,
            egress,
            scratch_size,
            overlay_clone_mode,
            vcpu_count,
            mem_size_mib,
            writeback,
            tty,
            interactive,
            warm,
            argv,
            json,
        ),

        Cmd::Preflight => cmds::cmd_preflight(json),

        Cmd::Quickstart(args) => cmds::cmd_quickstart(args, json),

        Cmd::Install(args) => cmds::cmd_install(args, json),

        Cmd::InstallStatus(args) => cmds::cmd_install_status(args, json),

        Cmd::Update(args) => cmds::cmd_update(args, json),

        Cmd::Inspect { ref vm_id } => cmds_walk::cmd_inspect(vm_id, json),

        Cmd::Logs {
            ref vm_id,
            follow,
            ref request_id,
            ref since,
        } => {
            cmds_walk::logs::cmd_logs(vm_id, follow, request_id.as_deref(), since.as_deref(), json)
        }

        Cmd::List => cmds_walk::cmd_list(json),

        Cmd::Env => cmds::cmd_env(json),

        Cmd::Cleanup { force } => cmds::cmd_cleanup(force, json),

        Cmd::Config {
            action: ConfigAction::Show,
        } => cmds::cmd_config_show(json),

        Cmd::Warm { action } => cmds::cmd_warm(action, json),

        Cmd::Image { action } => cmds::cmd_image(action, json),

        Cmd::Template { action } => cmds::cmd_template(action, json),

        Cmd::Version => cmds::cmd_version(json),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn dispatches_warm_status_without_subprocess() {
        let cli = Cli::try_parse_from(["m80", "warm", "status"]).unwrap();
        // warm status with no running owner returns 0 (renders "unavailable" status).
        let code = run(cli).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn dispatches_version_without_subprocess() {
        let cli = Cli::try_parse_from(["m80", "version"]).unwrap();
        let code = run(cli).unwrap();
        assert_eq!(code, 0);
    }
}
