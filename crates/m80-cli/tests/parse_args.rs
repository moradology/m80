//! Argv parse tests: every documented invocation must parse correctly.
//!
//! Uses `Cli::try_parse_from` so no subprocess is spawned.
//!
//! Behavior capture: bead m80-lt15.1 (CLI command contract).

use clap::Parser;
use m80_cli::{Cli, Cmd, ConfigAction, EgressMode, QuickstartArgs, WarmAction, WritebackMode};

// ---- run ----

#[test]
fn parse_run_defaults_to_process_wrapper_contract() {
    let cli = Cli::try_parse_from(["m80", "run", "--", "/bin/echo", "hi"]).unwrap();
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
            vcpu_count,
            mem_size_mib,
            writeback,
            tty,
            interactive,
            warm,
            argv,
        } => {
            assert!(profile.is_none());
            assert!(workspace.is_none());
            assert!(cwd.is_none());
            assert!(env.is_empty());
            assert!(secret_env.is_empty());
            assert!(!stdin);
            assert_eq!(egress, EgressMode::Outbound);
            assert!(scratch_size.is_none());
            assert!(vcpu_count.is_none());
            assert!(mem_size_mib.is_none());
            assert_eq!(writeback, WritebackMode::Never);
            assert!(!tty);
            assert!(!interactive);
            assert!(!warm);
            assert_eq!(argv, vec!["/bin/echo", "hi"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_visibility_and_exec_options() {
    let cli = Cli::try_parse_from([
        "m80",
        "run",
        "--workspace",
        "/tmp/ws",
        "--cwd",
        "/work",
        "--env",
        "FOO=bar",
        "--env",
        "EMPTY=",
        "--secret-env",
        "ANTHROPIC_API_KEY",
        "--stdin",
        "--egress",
        "none",
        "--scratch-size",
        "1048576",
        "--vcpu-count",
        "2",
        "--mem-size-mib",
        "512",
        "--",
        "/usr/bin/env",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Run {
            workspace,
            cwd,
            env,
            secret_env,
            stdin,
            egress,
            scratch_size,
            vcpu_count,
            mem_size_mib,
            argv,
            ..
        } => {
            assert_eq!(workspace, Some(std::path::PathBuf::from("/tmp/ws")));
            assert_eq!(cwd.as_deref(), Some("/work"));
            assert_eq!(env, vec!["FOO=bar", "EMPTY="]);
            assert_eq!(secret_env, vec!["ANTHROPIC_API_KEY"]);
            assert!(stdin);
            assert_eq!(egress, EgressMode::None);
            assert_eq!(scratch_size, Some(1_048_576));
            assert_eq!(vcpu_count, Some(2));
            assert_eq!(mem_size_mib, Some(512));
            assert_eq!(argv, vec!["/usr/bin/env"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_runtime_profile_shape() {
    let cli = Cli::try_parse_from(["m80", "run", "--profile", "ubuntu-dev", "--", "bash"]).unwrap();
    match cli.subcommand {
        Cmd::Run { profile, argv, .. } => {
            assert_eq!(profile.as_deref(), Some("ubuntu-dev"));
            assert_eq!(argv, vec!["bash"]);
        }
        _ => panic!("expected Run"),
    }
}


#[test]
fn parse_run_pty_flag_shape() {
    let cli = Cli::try_parse_from(["m80", "run", "-t", "-i", "--", "bash"]).unwrap();
    match cli.subcommand {
        Cmd::Run {
            tty,
            interactive,
            argv,
            ..
        } => {
            assert!(tty);
            assert!(interactive);
            assert_eq!(argv, vec!["bash"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_warm_flag_shape() {
    let cli = Cli::try_parse_from(["m80", "run", "--warm", "--", "bash"]).unwrap();
    match cli.subcommand {
        Cmd::Run { warm, argv, .. } => {
            assert!(warm);
            assert_eq!(argv, vec!["bash"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_writeback_shape() {
    let cli = Cli::try_parse_from([
        "m80",
        "run",
        "--writeback",
        "on-success",
        "--",
        "make",
        "test",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Run { writeback, argv, .. } => {
            assert_eq!(writeback, WritebackMode::OnSuccess);
            assert_eq!(argv, vec!["make", "test"]);
        }
        _ => panic!("expected Run"),
    }
}

#[test]
fn parse_run_requires_command_after_separator() {
    let result = Cli::try_parse_from(["m80", "run"]);
    assert!(result.is_err(), "expected m80 run without argv to fail");
}

#[test]
fn parse_run_requires_explicit_separator_before_command() {
    let result = Cli::try_parse_from(["m80", "run", "/bin/echo"]);
    assert!(
        result.is_err(),
        "expected m80 run command argv to require `--`"
    );
}

#[test]
fn parse_run_rejects_removed_noegress_spelling() {
    let result = Cli::try_parse_from(["m80", "run", "--egress", "noegress", "--", "true"]);
    assert!(
        result.is_err(),
        "egress values are exactly `none` or `outbound`"
    );
}

#[test]
fn parse_rejects_in_core_agent_subcommand() {
    let result = Cli::try_parse_from(["m80", "agent", "run", "--tool", "bash"]);
    assert!(
        result.is_err(),
        "agent tool facades belong in an external adapter, not m80-cli"
    );
}

#[test]
fn parse_run_rejects_tool_catalog_flag() {
    let result = Cli::try_parse_from(["m80", "run", "--tool", "bash", "--", "true"]);
    assert!(
        result.is_err(),
        "m80 run accepts process-wrapper flags, not adapter tool catalog flags"
    );
}

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

// ---- quickstart ----

#[test]
fn parse_quickstart_required_artifact_url() {
    let cli = Cli::try_parse_from([
        "m80",
        "quickstart",
        "--artifact-url",
        "file:///tmp/m80-artifacts.tar.gz",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Quickstart(QuickstartArgs {
            artifact_url,
            artifact_dir,
            run_root,
            no_run,
        }) => {
            assert_eq!(artifact_url, "file:///tmp/m80-artifacts.tar.gz");
            assert!(artifact_dir.is_none());
            assert!(run_root.is_none());
            assert!(!no_run);
        }
        _ => panic!("expected Quickstart"),
    }
}

#[test]
fn parse_quickstart_install_only_shape() {
    let cli = Cli::try_parse_from([
        "m80",
        "quickstart",
        "--artifact-url",
        "https://example.invalid/m80.tar.gz",
        "--artifact-dir",
        "/tmp/artifacts",
        "--run-root",
        "/tmp/run",
        "--no-run",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Quickstart(args) => {
            assert_eq!(args.artifact_url, "https://example.invalid/m80.tar.gz");
            assert_eq!(
                args.artifact_dir,
                Some(std::path::PathBuf::from("/tmp/artifacts"))
            );
            assert_eq!(args.run_root, Some(std::path::PathBuf::from("/tmp/run")));
            assert!(args.no_run);
        }
        _ => panic!("expected Quickstart"),
    }
}

#[test]
fn parse_quickstart_requires_artifact_url() {
    let result = Cli::try_parse_from(["m80", "quickstart", "--no-run"]);
    assert!(result.is_err(), "quickstart requires --artifact-url");
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

// ---- logs ----

#[test]
fn parse_logs_filter_shape() {
    let cli = Cli::try_parse_from([
        "m80",
        "logs",
        "vm-abc",
        "--follow",
        "--request-id",
        "req-1",
        "--since",
        "1970-01-01T00:00:01Z",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Logs {
            vm_id,
            follow,
            request_id,
            since,
        } => {
            assert_eq!(vm_id, "vm-abc");
            assert!(follow);
            assert_eq!(request_id.as_deref(), Some("req-1"));
            assert_eq!(since.as_deref(), Some("1970-01-01T00:00:01Z"));
        }
        _ => panic!("expected Logs"),
    }
}

// ---- list ----

#[test]
fn parse_list() {
    let cli = Cli::try_parse_from(["m80", "list"]).unwrap();
    assert!(matches!(cli.subcommand, Cmd::List));
}

// ---- env ----

#[test]
fn parse_env() {
    let cli = Cli::try_parse_from(["m80", "env"]).unwrap();
    assert!(matches!(cli.subcommand, Cmd::Env));
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

// ---- warm ----

#[test]
fn parse_warm_requires_action() {
    let result = Cli::try_parse_from(["m80", "warm"]);
    assert!(result.is_err());
}

#[test]
fn parse_warm_enable_shape() {
    let cli = Cli::try_parse_from([
        "m80",
        "warm",
        "enable",
        "--size",
        "2",
        "--egress",
        "none",
        "--profile",
        "minimal",
    ])
    .unwrap();
    match cli.subcommand {
        Cmd::Warm {
            action: WarmAction::Enable(args),
        } => {
            assert_eq!(args.size, 2);
            assert_eq!(args.egress, EgressMode::None);
            assert_eq!(args.profile.as_deref(), Some("minimal"));
        }
        _ => panic!("expected Warm Enable"),
    }
}

#[test]
fn parse_warm_status_drain_disable_shapes() {
    let status = Cli::try_parse_from(["m80", "warm", "status", "--profile", "minimal"]).unwrap();
    assert!(matches!(
        status.subcommand,
        Cmd::Warm {
            action: WarmAction::Status { .. }
        }
    ));

    let drain = Cli::try_parse_from(["m80", "warm", "drain"]).unwrap();
    assert!(matches!(
        drain.subcommand,
        Cmd::Warm {
            action: WarmAction::Drain
        }
    ));

    let disable = Cli::try_parse_from(["m80", "warm", "disable"]).unwrap();
    assert!(matches!(
        disable.subcommand,
        Cmd::Warm {
            action: WarmAction::Disable
        }
    ));
}

// ---- version ----

#[test]
fn parse_version() {
    let cli = Cli::try_parse_from(["m80", "version"]).unwrap();
    assert!(matches!(cli.subcommand, Cmd::Version));
}

// ---- removed VM-front-door commands fail ----

#[test]
fn parse_removed_launch_subcommand_fails() {
    let result = Cli::try_parse_from(["m80", "launch", "--", "true"]);
    assert!(result.is_err(), "launch is not a compatibility alias");
}

#[test]
fn parse_removed_exec_subcommand_fails() {
    let result = Cli::try_parse_from(["m80", "exec", "vm-abc", "--", "true"]);
    assert!(result.is_err(), "exec is not a compatibility alias");
}

#[test]
fn parse_removed_stop_subcommand_fails() {
    let result = Cli::try_parse_from(["m80", "stop", "vm-abc"]);
    assert!(result.is_err(), "stop is not part of the process facade");
}

#[test]
fn parse_removed_snapshot_subcommand_fails() {
    let result = Cli::try_parse_from(["m80", "snapshot", "capture", "vm-abc"]);
    assert!(result.is_err(), "snapshot is not a process facade command");
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
