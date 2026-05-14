use super::{
    build_process_env, format_config_json, format_config_table, host_feature_config_from_effective,
    network_policy_for_egress, parse_env, render_preflight_result, run_request, run_stream,
    sandbox_config_for_run, should_writeback, validate_run_flags,
};
use crate::args::{EgressMode, WritebackMode};
use crate::errors::EXIT_PREFLIGHT;
use crate::json;
use m80_firecracker::{ConfigSource, EffectiveConfig, EffectiveField, NetworkPolicy};
use m80_preflight::{CgroupPreflightMode, CheckRow, Discovery, PreflightError};
use m80_proto::ExecStatus;

fn fake_discovery() -> Discovery {
    let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
    let rootfs_path = rootfs.path().to_path_buf();
    let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
    let mut d = Discovery {
        firecracker_bin: "/tmp/firecracker".into(),
        jailer_bin: "/tmp/jailer".into(),
        jailer_harden_bin: "/tmp/m80-jailer-harden".into(),
        kernel: "/tmp/vmlinux".into(),
        rootfs: "/tmp/rootfs.ext4".into(),
        pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
        manifest: fake_manifest(),
        run_root: "/tmp/m80-run".into(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: Vec::new(),
    };
    d.report = vec![CheckRow {
        label: "kvm".to_owned(),
        passed: true,
        detail: "fixture".to_owned(),
    }];
    d
}

fn fake_manifest() -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest::new(
        "/tmp/m80-guestd".into(),
        "0".repeat(64),
        "v1.0.0".to_owned(),
        52,
        m80_image_manifest::ImageKind::Minimal,
        "/tmp/vmlinux".into(),
        "1".repeat(64),
        m80_image_manifest::KernelKind::Stock,
        None,
        "/tmp/rootfs.ext4".into(),
        "2".repeat(64),
        "M80_READY".to_owned(),
        m80_image_manifest::RootfsFormat::Ext4,
        None,
        None,
    )
}

#[test]
fn preflight_error_uses_shared_error_mapping() {
    let code = render_preflight_result(
        Err(PreflightError::KvmUnavailable {
            path: "/dev/kvm".into(),
        }),
        false,
    );
    assert_eq!(code, EXIT_PREFLIGHT);
}

#[test]
fn effective_cgroup_mode_maps_to_preflight_config() {
    let config = EffectiveConfig {
        fields: vec![EffectiveField {
            name: "cgroup_mode".to_owned(),
            value: "disabled".to_owned(),
            source: ConfigSource::Env,
        }],
    };

    let host_features = host_feature_config_from_effective(&config).unwrap();

    assert_eq!(host_features.cgroup_mode, CgroupPreflightMode::Disabled);
}

#[test]
fn effective_jail_identity_maps_to_preflight_config() {
    let config = EffectiveConfig {
        fields: vec![
            EffectiveField {
                name: "jail_uid".to_owned(),
                value: "3100".to_owned(),
                source: ConfigSource::Env,
            },
            EffectiveField {
                name: "jail_gid".to_owned(),
                value: "3200".to_owned(),
                source: ConfigSource::Env,
            },
        ],
    };

    let host_features = host_feature_config_from_effective(&config).unwrap();

    assert_eq!(host_features.jail_uid, 3100);
    assert_eq!(host_features.jail_gid, 3200);
}

#[test]
fn effective_max_concurrent_vms_maps_to_preflight_conntrack_sizing() {
    let config = EffectiveConfig {
        fields: vec![EffectiveField {
            name: "max_concurrent_vms".to_owned(),
            value: "12".to_owned(),
            source: ConfigSource::Env,
        }],
    };

    let host_features = host_feature_config_from_effective(&config).unwrap();

    assert_eq!(host_features.expected_concurrent_vms, 12);
}

#[test]
fn invalid_effective_jail_identity_stays_typed_preflight_error() {
    let config = EffectiveConfig {
        fields: vec![EffectiveField {
            name: "jail_uid".to_owned(),
            value: "not-a-uid".to_owned(),
            source: ConfigSource::Env,
        }],
    };

    let err = host_feature_config_from_effective(&config).unwrap_err();

    match err {
        PreflightError::InvalidJailIdentity { field, value } => {
            assert_eq!(field, "jail_uid");
            assert_eq!(value, "not-a-uid");
        }
        other => panic!("expected InvalidJailIdentity, got {other:?}"),
    }
}

#[test]
fn invalid_effective_cgroup_mode_stays_typed_preflight_error() {
    let config = EffectiveConfig {
        fields: vec![EffectiveField {
            name: "cgroup_mode".to_owned(),
            value: "legacy".to_owned(),
            source: ConfigSource::Env,
        }],
    };

    let err = host_feature_config_from_effective(&config).unwrap_err();

    match err {
        PreflightError::InvalidCgroupMode { actual } => assert_eq!(actual, "legacy"),
        other => panic!("expected InvalidCgroupMode, got {other:?}"),
    }
}

#[test]
fn run_egress_mode_maps_to_sandbox_network_policy() {
    assert_eq!(
        network_policy_for_egress(EgressMode::None),
        NetworkPolicy::NoEgress
    );
    assert_eq!(
        network_policy_for_egress(EgressMode::Outbound),
        NetworkPolicy::AllowOutbound { exceptions: vec![] }
    );
}

#[test]
fn run_workspace_and_scratch_map_to_sandbox_config() {
    let config = sandbox_config_for_run(
        Some("/tmp/m80-ws".into()),
        EgressMode::None,
        Some(64 * 1024 * 1024),
        Some(2),
        Some(768),
        "req-test".to_owned(),
    );

    assert_eq!(
        config.workspace.as_deref(),
        Some(std::path::Path::new("/tmp/m80-ws"))
    );
    assert_eq!(config.network, NetworkPolicy::NoEgress);
    assert_eq!(config.overlay_size_bytes, 64 * 1024 * 1024);
    assert_eq!(config.vcpu_count, Some(2));
    assert_eq!(config.mem_size_mib, Some(768));
    assert!(config.idle_timeout.is_none());
    assert_eq!(config.request_id.as_deref(), Some("req-test"));
}

#[test]
fn run_defaults_to_no_workspace_and_default_overlay_size() {
    let config = sandbox_config_for_run(
        None,
        EgressMode::Outbound,
        None,
        None,
        None,
        "req-default".to_owned(),
    );

    assert!(config.workspace.is_none());
    assert_eq!(
        config.network,
        NetworkPolicy::AllowOutbound { exceptions: vec![] }
    );
    assert_eq!(config.overlay_size_bytes, 512 * 1024 * 1024);
    assert!(config.vcpu_count.is_none());
    assert!(config.mem_size_mib.is_none());
    assert_eq!(config.request_id.as_deref(), Some("req-default"));
}

#[test]
fn warm_run_rejects_cold_boot_resource_sizing_flags() {
    let vcpu_err = validate_run_flags(false, false, false, true, false, true, false, false)
        .expect_err("--warm --vcpu-count must fail");
    assert!(
        vcpu_err
            .to_string()
            .contains("--warm is incompatible with --vcpu-count"),
        "unexpected error: {vcpu_err}"
    );

    let mem_err = validate_run_flags(false, false, false, true, false, false, true, false)
        .expect_err("--warm --mem-size-mib must fail");
    assert!(
        mem_err
            .to_string()
            .contains("--warm is incompatible with --mem-size-mib"),
        "unexpected error: {mem_err}"
    );
}

#[test]
fn run_cwd_env_and_stdin_map_to_exec_request() {
    let args = vec!["one".to_owned(), "two".to_owned()];
    let env = Some(vec![
        ("FOO".to_owned(), "bar".to_owned()),
        ("EMPTY".to_owned(), String::new()),
    ]);
    let request = run_request::exec_request_for_run(
        "/usr/bin/env",
        &args,
        env.clone(),
        Some("/workspace/subdir".to_owned()),
        Some(b"stdin bytes".to_vec()),
    );

    assert_eq!(request.program, "/usr/bin/env");
    assert_eq!(request.args, args);
    assert_eq!(request.env, env);
    assert_eq!(request.cwd.as_deref(), Some("/workspace/subdir"));
    assert_eq!(request.stdin.as_deref(), Some(&b"stdin bytes"[..]));
    assert!(request.timeout_ms.is_none());
}

#[test]
fn run_cwd_env_and_terminal_size_map_to_pty_request() {
    let args = vec!["one".to_owned(), "two".to_owned()];
    let env = Some(vec![("TERM".to_owned(), "xterm-256color".to_owned())]);
    let request =
        run_request::pty_request_for_run("/usr/bin/vim", &args, env, Some("/workspace".to_owned()));

    assert_eq!(request.program, "/usr/bin/vim");
    assert_eq!(request.args, args);
    let request_env = request.env.as_ref().expect("pty request env");
    assert!(request_env.contains(&("TERM".to_owned(), "xterm-256color".to_owned())));
    assert_eq!(
        request_env.iter().filter(|(key, _)| key == "TERM").count(),
        1
    );
    assert_eq!(request.cwd.as_deref(), Some("/workspace"));
    assert!(request.timeout_ms.is_none());
    assert!(request.size.rows > 0);
    assert!(request.size.cols > 0);
}

#[test]
fn run_passthrough_copies_streaming_chunks_without_crossing_streams() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    run_stream::copy_guest_chunk(
        m80_firecracker::ExecChunk::Stdout {
            seq: 0,
            bytes: vec![0xff, b'o'],
        },
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    run_stream::copy_guest_chunk(
        m80_firecracker::ExecChunk::Stderr {
            seq: 0,
            bytes: vec![0xfe, b'e'],
        },
        &mut stdout,
        &mut stderr,
    )
    .unwrap();

    assert_eq!(stdout, vec![0xff, b'o']);
    assert_eq!(stderr, vec![0xfe, b'e']);
}

#[test]
fn cancelled_run_maps_to_conventional_signal_exit_code() {
    assert_eq!(
        run_stream::process_exit_code(ExecStatus::Cancelled, None, Some(2)),
        130
    );
    assert_eq!(
        run_stream::process_exit_code(ExecStatus::Cancelled, None, Some(15)),
        143
    );
}

#[test]
fn non_cancelled_run_preserves_guest_exit_code_even_if_signal_raced_late() {
    assert_eq!(
        run_stream::process_exit_code(ExecStatus::Completed, Some(7), Some(2)),
        7
    );
    assert_eq!(
        run_stream::process_exit_code(ExecStatus::Failed, None, Some(15)),
        1
    );
}

#[test]
fn run_env_parser_keeps_empty_values_and_rejects_invalid_shape() {
    let parsed = parse_env(&["FOO=bar".to_owned(), "EMPTY=".to_owned()]).unwrap();
    assert_eq!(
        parsed,
        Some(vec![
            ("FOO".to_owned(), "bar".to_owned()),
            ("EMPTY".to_owned(), String::new()),
        ])
    );

    let missing_separator = parse_env(&["FOO".to_owned()]).unwrap_err();
    assert!(missing_separator
        .to_string()
        .contains("environment override must be KEY=VAL"));

    let empty_key = parse_env(&["=value".to_owned()]).unwrap_err();
    assert!(empty_key
        .to_string()
        .contains("environment override key must not be empty"));
}

#[test]
fn secret_env_requires_named_existing_host_variable() {
    let missing =
        build_process_env(&Vec::new(), &vec!["M80_TEST_SECRET_MISSING".to_owned()]).unwrap_err();
    assert!(missing
        .to_string()
        .contains("secret env `M80_TEST_SECRET_MISSING` is not set"));

    let bad_key = build_process_env(&Vec::new(), &vec!["BAD=KEY".to_owned()]).unwrap_err();
    assert!(bad_key
        .to_string()
        .contains("secret env key must be a variable name"));
}

#[test]
fn writeback_policy_decides_from_guest_exit() {
    assert!(!should_writeback(WritebackMode::Never, 0));
    assert!(!should_writeback(WritebackMode::Never, 1));
    assert!(should_writeback(WritebackMode::OnSuccess, 0));
    assert!(!should_writeback(WritebackMode::OnSuccess, 1));
    assert!(should_writeback(WritebackMode::Always, 0));
    assert!(should_writeback(WritebackMode::Always, 42));
}

#[test]
fn preflight_success_fixture_needs_no_kvm() {
    let code = render_preflight_result(Ok(fake_discovery()), false);
    assert_eq!(code, 0);
}

#[test]
fn preflight_json_formats_report_rows_without_kvm() {
    let rows = fake_discovery().report;
    let json = json::to_pretty(rows.as_slice());
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"][0]["label"], "kvm");
    assert_eq!(parsed["data"][0]["passed"], true);
}

#[test]
fn cleanup_json_formats_status_without_backend() {
    let json = json::to_pretty(&serde_json::json!({ "status": "ok" }));
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"]["status"], "ok");
}

#[test]
fn config_table_formats_effective_fields_without_preflight() {
    let effective = EffectiveConfig {
        fields: vec![
            EffectiveField {
                name: "run_root".to_owned(),
                value: "/tmp/m80".to_owned(),
                source: ConfigSource::Env,
            },
            EffectiveField {
                name: "max_concurrent_vms".to_owned(),
                value: "4".to_owned(),
                source: ConfigSource::Default,
            },
        ],
    };

    let table = format_config_table(&effective);
    assert!(table.contains("FIELD"));
    assert!(table.contains("run_root"));
    assert!(table.contains("/tmp/m80"));
    assert!(table.contains("Env"));
    assert!(table.contains("max_concurrent_vms"));
}

#[test]
fn config_json_formats_effective_fields_without_preflight() {
    let effective = EffectiveConfig {
        fields: vec![EffectiveField {
            name: "run_root".to_owned(),
            value: "/tmp/m80".to_owned(),
            source: ConfigSource::Env,
        }],
    };

    let json = format_config_json(&effective);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["data"]["fields"][0]["name"], "run_root");
    assert_eq!(parsed["data"]["fields"][0]["source"], "env");
}
