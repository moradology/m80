//! Argv parse tests: every documented invocation must parse correctly.
//!
//! Uses `Cli::try_parse_from` so no subprocess is spawned.
//!
//! Behavior capture: bead m80-4ef.3 (CLI argument parsing correctness).

use clap::Parser;
use m80_cli::{Cli, Cmd, ConfigAction};
use m80_firecracker::NetworkPolicy;

// ---- preflight ----

#[test]
fn parse_preflight() {
    let cli = Cli::try_parse_from(["m80", "preflight"]).unwrap();
    assert!(!cli.json);
    assert!(matches!(cli.subcommand, Cmd::Preflight));
}

#[test]
fn parse_preflight_json() {
    let cli = Cli::try_parse_from(["m80", "--json", "preflight"]).unwrap();
    assert!(cli.json);
}

// ---- launch ----

#[test]
fn parse_launch_defaults() {
    let cli = Cli::try_parse_from(["m80", "launch"]).unwrap();
    match cli.subcommand {
        Cmd::Launch {
            workspace,
            network,
            id,
            exec,
        } => {
            assert!(workspace.is_none());
            assert_eq!(network, NetworkPolicy::NoEgress);
            assert!(id.is_none());
            assert!(exec.is_empty());
        }
        _ => panic!("expected Launch"),
    }
}

#[test]
fn parse_launch_with_workspace() {
    let cli = Cli::try_parse_from(["m80", "launch", "--workspace", "/tmp/ws"]).unwrap();
    match cli.subcommand {
        Cmd::Launch { workspace, .. } => {
            assert_eq!(workspace, Some(std::path::PathBuf::from("/tmp/ws")));
        }
        _ => panic!("expected Launch"),
    }
}

#[test]
fn parse_launch_with_network_outbound() {
    let cli = Cli::try_parse_from(["m80", "launch", "--network", "outbound"]).unwrap();
    match cli.subcommand {
        Cmd::Launch { network, .. } => {
            assert!(matches!(network, NetworkPolicy::AllowOutbound { .. }));
        }
        _ => panic!("expected Launch"),
    }
}

#[test]
fn parse_launch_with_id() {
    let cli = Cli::try_parse_from(["m80", "launch", "--id", "my-vm-1"]).unwrap();
    match cli.subcommand {
        Cmd::Launch { id, .. } => {
            assert_eq!(id, Some("my-vm-1".to_owned()));
        }
        _ => panic!("expected Launch"),
    }
}

#[test]
fn parse_launch_single_shot_exec() {
    let cli = Cli::try_parse_from(["m80", "launch", "--", "/bin/sh", "-c", "echo hi"]).unwrap();
    match cli.subcommand {
        Cmd::Launch { exec, .. } => {
            assert_eq!(exec, vec!["/bin/sh", "-c", "echo hi"]);
        }
        _ => panic!("expected Launch"),
    }
}

#[test]
fn parse_launch_network_case_insensitive() {
    let cli = Cli::try_parse_from(["m80", "launch", "--network", "NoEgress"]).unwrap();
    match cli.subcommand {
        Cmd::Launch { network, .. } => {
            assert_eq!(network, NetworkPolicy::NoEgress);
        }
        _ => panic!("expected Launch"),
    }
}

// ---- exec ----

#[test]
fn parse_exec_basic() {
    let cli = Cli::try_parse_from(["m80", "exec", "vm-abc", "--", "ls", "-la"]).unwrap();
    match cli.subcommand {
        Cmd::Exec {
            vm_id,
            argv,
            cwd,
            env,
            timeout_ms,
        } => {
            assert_eq!(vm_id, "vm-abc");
            assert_eq!(argv, vec!["ls", "-la"]);
            assert!(cwd.is_none());
            assert!(env.is_empty());
            assert!(timeout_ms.is_none());
        }
        _ => panic!("expected Exec"),
    }
}

#[test]
fn parse_exec_with_cwd_env_timeout() {
    let cli = Cli::try_parse_from([
        "m80",
        "exec",
        "vm-xyz",
        "--cwd",
        "/app",
        "--env",
        "FOO=bar",
        "--env",
        "BAZ=qux",
        "--timeout-ms",
        "5000",
        "--",
        "/usr/bin/env",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Exec {
            vm_id,
            argv,
            cwd,
            env,
            timeout_ms,
        } => {
            assert_eq!(vm_id, "vm-xyz");
            assert_eq!(argv, vec!["/usr/bin/env"]);
            assert_eq!(cwd.as_deref(), Some("/app"));
            assert_eq!(env, vec!["FOO=bar", "BAZ=qux"]);
            assert_eq!(timeout_ms, Some(5000));
        }
        _ => panic!("expected Exec"),
    }
}

// ---- stop ----

#[test]
fn parse_stop_basic() {
    let cli = Cli::try_parse_from(["m80", "stop", "vm-abc"]).unwrap();
    match cli.subcommand {
        Cmd::Stop {
            vm_id,
            extract_changes,
        } => {
            assert_eq!(vm_id, "vm-abc");
            assert!(extract_changes.is_none());
        }
        _ => panic!("expected Stop"),
    }
}

#[test]
fn parse_stop_with_extract() {
    let cli =
        Cli::try_parse_from(["m80", "stop", "vm-abc", "--extract-changes", "/tmp/out"]).unwrap();
    match cli.subcommand {
        Cmd::Stop {
            extract_changes, ..
        } => {
            assert_eq!(extract_changes, Some(std::path::PathBuf::from("/tmp/out")));
        }
        _ => panic!("expected Stop"),
    }
}

// ---- inspect ----

#[test]
fn parse_inspect() {
    let cli = Cli::try_parse_from(["m80", "inspect", "vm-abc"]).unwrap();
    match cli.subcommand {
        Cmd::Inspect { vm_id } => {
            assert_eq!(vm_id, "vm-abc");
        }
        _ => panic!("expected Inspect"),
    }
}

// ---- list ----

#[test]
fn parse_list() {
    let cli = Cli::try_parse_from(["m80", "list"]).unwrap();
    assert!(matches!(cli.subcommand, Cmd::List));
}

// ---- cleanup ----

#[test]
fn parse_cleanup_no_force() {
    let cli = Cli::try_parse_from(["m80", "cleanup"]).unwrap();
    match cli.subcommand {
        Cmd::Cleanup { force } => assert!(!force),
        _ => panic!("expected Cleanup"),
    }
}

#[test]
fn parse_cleanup_force() {
    let cli = Cli::try_parse_from(["m80", "cleanup", "--force"]).unwrap();
    match cli.subcommand {
        Cmd::Cleanup { force } => assert!(force),
        _ => panic!("expected Cleanup"),
    }
}

// ---- config show ----

#[test]
fn parse_config_show() {
    let cli = Cli::try_parse_from(["m80", "config", "show"]).unwrap();
    match cli.subcommand {
        Cmd::Config {
            action: ConfigAction::Show,
        } => {}
        _ => panic!("expected Config Show"),
    }
}

// ---- version ----

#[test]
fn parse_version() {
    let cli = Cli::try_parse_from(["m80", "version"]).unwrap();
    assert!(matches!(cli.subcommand, Cmd::Version));
}

// ---- unknown subcommand fails ----

#[test]
fn parse_unknown_subcommand_fails() {
    let result = Cli::try_parse_from(["m80", "frobnicate"]);
    assert!(
        result.is_err(),
        "expected parse failure for unknown subcommand"
    );
}

// ---- --json is global ----

#[test]
fn json_flag_after_subcommand_parses_and_sets_json() {
    // `--json` is a top-level global flag; clap accepts it both before
    // and after the subcommand. Verify the after-subcommand position
    // both parses successfully AND sets the flag (the prior assertion
    // was a tautology — `is_ok() || is_err()` proves nothing).
    let cli = Cli::try_parse_from(["m80", "version", "--json"]).unwrap();
    assert!(
        cli.json,
        "--json after subcommand should still set the flag"
    );
}

#[test]
fn json_flag_before_subcommand_works() {
    let cli = Cli::try_parse_from(["m80", "--json", "version"]).unwrap();
    assert!(cli.json);
}
