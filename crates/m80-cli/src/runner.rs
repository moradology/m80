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
            allow_host,
            allow_cidr,
            mount_config,
            scratch_size,
            writeback,
            keep_on_failure,
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
            allow_host,
            allow_cidr,
            mount_config,
            scratch_size,
            writeback,
            keep_on_failure,
            tty,
            interactive,
            warm,
            argv,
            json,
        ),

        Cmd::Preflight => cmds::cmd_preflight(json),

        Cmd::Quickstart(args) => cmds::cmd_quickstart(args, json),

        Cmd::Inspect { ref vm_id } => cmds_walk::cmd_inspect(vm_id, json),

        Cmd::List => cmds_walk::cmd_list(json),

        Cmd::Cleanup { force } => cmds::cmd_cleanup(force, json),

        Cmd::Config {
            action: ConfigAction::Show,
        } => cmds::cmd_config_show(json),

        Cmd::Warm { action } => cmds::cmd_warm(action, json),

        Cmd::Version => cmds::cmd_version(json),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::args::WarmAction;
    use crate::errors::EXIT_NOT_IMPLEMENTED;

    #[test]
    fn dispatches_feature_gap_without_subprocess() {
        let cli =
            Cli::try_parse_from(["m80", "warm", "enable", "--system", "--size", "1"]).unwrap();
        let code = run(cli).unwrap();
        assert_eq!(code, EXIT_NOT_IMPLEMENTED);
    }

    #[test]
    fn dispatches_warm_status_without_subprocess() {
        let cli = Cli::try_parse_from(["m80", "warm", "status"]).unwrap();
        let Cmd::Warm {
            action: WarmAction::Status { .. },
        } = &cli.subcommand
        else {
            panic!("expected warm status");
        };
    }

    #[test]
    fn dispatches_version_without_subprocess() {
        let cli = Cli::try_parse_from(["m80", "version"]).unwrap();
        let code = run(cli).unwrap();
        assert_eq!(code, 0);
    }
}
