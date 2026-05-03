//! `m80` — host-side CLI. Thin shell over `m80-firecracker`.
//!
//! See `README.md` for the contract.
//! Behavior captures: dossier `09-cli-shape.md`; beads `m80-4ef.3.*`
//! (error → exit-code mapping) and `m80-v7t.*` (config loading order,
//! env-var schema).
//!
//! # Module layout
//!
//! - `args`   — clap-derive structs (`Cli`, `Cmd`, `ConfigAction`)
//! - `config` — precedence-chain config loading (bead m80-v7t.1/2)
//! - `errors` — FcError → exit-code map + JSON envelope (bead m80-4ef.3)
//! - `cmds`   — per-subcommand logic; returns `i32` exit code
//!
//! All modules are declared in `lib.rs`; this file is the entry point only.

use clap::Parser;

use m80_cli::args::{Cli, Cmd, ConfigAction};
use m80_cli::cmds;
use m80_cli::cmds_walk;

/// Dispatch subcommand and return the exit code.
fn run(cli: Cli) -> anyhow::Result<i32> {
    let json = cli.json;
    match cli.subcommand {
        Cmd::Preflight => cmds::cmd_preflight(json),

        Cmd::Launch {
            workspace,
            network,
            id,
            exec,
        } => cmds::cmd_launch(workspace, network, id, exec, json),

        Cmd::Exec {
            ref vm_id,
            ref argv,
            ..
        } => cmds::cmd_exec(vm_id, argv, json),

        Cmd::Stop {
            ref vm_id,
            ref extract_changes,
        } => cmds_walk::cmd_stop(vm_id, extract_changes.as_deref(), json),

        Cmd::Inspect { ref vm_id } => cmds_walk::cmd_inspect(vm_id, json),

        Cmd::List => cmds_walk::cmd_list(json),

        Cmd::Cleanup { force } => cmds::cmd_cleanup(force, json),

        Cmd::Config {
            action: ConfigAction::Show,
        } => cmds::cmd_config_show(json),

        Cmd::Version => cmds::cmd_version(json),
    }
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(exit_code);
}
