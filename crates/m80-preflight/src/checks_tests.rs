use super::*;

#[test]
fn cgroup_mode_parser_accepts_disabled() {
    let mode = parse_cgroup_mode("disabled").unwrap();

    assert_eq!(mode, CgroupPreflightMode::Disabled);
}

#[test]
fn manifest_firecracker_train_accepts_matching_version() {
    check_manifest_firecracker_train("v1.15.1", "v1.15.1").unwrap();
}

#[test]
fn manifest_firecracker_train_rejects_mismatch() {
    let err = check_manifest_firecracker_train("v1.15.1", "v1.14.4").unwrap_err();

    match err {
        PreflightError::FirecrackerVersionMismatch {
            expected,
            actual,
            policy_source,
        } => {
            assert_eq!(expected, "v1.15.1");
            assert_eq!(actual, "v1.14.4");
            assert_eq!(
                policy_source,
                "crates/m80-preflight/src/firecracker_train.rs"
            );
        }
        other => panic!("expected FirecrackerVersionMismatch, got {other:?}"),
    }
}

#[test]
fn cgroup_mode_parser_accepts_unified_v2() {
    let mode = parse_cgroup_mode("unified-v2").unwrap();

    assert_eq!(mode, CgroupPreflightMode::UnifiedV2);
}

#[test]
fn cgroup_mode_parser_rejects_unknown_value() {
    let err = parse_cgroup_mode("legacy").unwrap_err();

    match err {
        PreflightError::InvalidCgroupMode { actual } => assert_eq!(actual, "legacy"),
        other => panic!("expected InvalidCgroupMode, got {other:?}"),
    }
}

#[test]
fn jail_id_parser_accepts_decimal_u32() {
    assert_eq!(parse_jail_id("jail_uid", "3000").unwrap(), 3000);
}

#[test]
fn jail_id_parser_rejects_non_u32() {
    let err = parse_jail_id("jail_uid", "not-a-uid").unwrap_err();

    match err {
        PreflightError::InvalidJailIdentity { field, value } => {
            assert_eq!(field, "jail_uid");
            assert_eq!(value, "not-a-uid");
        }
        other => panic!("expected InvalidJailIdentity, got {other:?}"),
    }
}

#[test]
fn host_kernel_floor_accepts_minimum_release() {
    classify_host_kernel_release("6.1.0").unwrap();
}

#[test]
fn host_kernel_floor_accepts_newer_distribution_release() {
    classify_host_kernel_release("6.17.0-22-generic").unwrap();
}

#[test]
fn host_kernel_floor_rejects_old_release() {
    let err = classify_host_kernel_release("5.15.0").unwrap_err();

    match err {
        PreflightError::HostKernelUnsupported { actual, minimum } => {
            assert_eq!(actual, "5.15.0");
            assert_eq!(minimum, "6.1");
        }
        other => panic!("expected HostKernelUnsupported, got {other:?}"),
    }
}

#[test]
fn host_kernel_floor_rejects_unparseable_release() {
    let err = classify_host_kernel_release("not-a-kernel").unwrap_err();

    assert!(matches!(err, PreflightError::HostKernelUnsupported { .. }));
}

#[test]
fn preflight_cgroup_v2_unavailability_typed() {
    let err = classify_cgroup_probe(
        CgroupPreflightMode::UnifiedV2,
        Err(m80_cgroup::CgroupError::UnsupportedHostMode),
    )
    .unwrap_err();

    assert!(matches!(err, PreflightError::CgroupV2Unavailable));
}

#[test]
fn disabled_cgroup_mode_skips_cgroup_v2_probe() {
    classify_cgroup_probe(
        CgroupPreflightMode::Disabled,
        Err(m80_cgroup::CgroupError::UnsupportedHostMode),
    )
    .unwrap();
}

#[test]
fn disabled_cgroup_mode_does_not_call_live_probe() {
    let mut report = Vec::new();

    check_cgroup_mode_with_probe(CgroupPreflightMode::Disabled, &mut report, || {
        panic!("disabled cgroup mode must not probe cgroup v2")
    })
    .unwrap();

    assert_eq!(report.len(), 1);
    assert_eq!(report[0].label, "Cgroup mode");
    assert_eq!(report[0].detail, "disabled");
}

#[test]
fn cgroup_v2_probe_errors_remain_typed_system_io() {
    let err = classify_cgroup_probe(
        CgroupPreflightMode::UnifiedV2,
        Err(m80_cgroup::CgroupError::ControllerNotEnabled("cpu")),
    )
    .unwrap_err();

    assert!(matches!(
        err,
        PreflightError::SystemIo {
            operation: "cgroup v2 probe",
            ..
        }
    ));
}

#[test]
fn jailer_identity_reports_existing_user_and_group() {
    let row = classify_jailer_identity(3000, 3000, Some("m80".to_owned()), Some("m80".to_owned()))
        .unwrap();

    assert_eq!(row.label, "Jailer identity");
    assert!(row.passed);
    assert_eq!(row.detail, "uid=3000 (m80), gid=3000 (m80)");
}

#[test]
fn jailer_identity_requires_existing_user() {
    let err = classify_jailer_identity(3000, 3000, None, Some("m80".to_owned())).unwrap_err();

    match err {
        PreflightError::JailIdentityUnavailable { field, id } => {
            assert_eq!(field, "jail_uid");
            assert_eq!(id, 3000);
        }
        other => panic!("expected JailIdentityUnavailable, got {other:?}"),
    }
}

#[test]
fn jailer_identity_requires_existing_group() {
    let err = classify_jailer_identity(3000, 3000, Some("m80".to_owned()), None).unwrap_err();

    match err {
        PreflightError::JailIdentityUnavailable { field, id } => {
            assert_eq!(field, "jail_gid");
            assert_eq!(id, 3000);
        }
        other => panic!("expected JailIdentityUnavailable, got {other:?}"),
    }
}

#[test]
fn preflight_missing_vsock_module_typed() {
    let err = classify_vsock_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

    assert!(matches!(err, PreflightError::VsockUnavailable));
}

#[test]
fn vhost_vsock_module_satisfies_vsock_preflight() {
    classify_vsock_availability(&HashSet::from(["tap", "bridge", "vhost_vsock"]), false).unwrap();
}

#[test]
fn vhost_vsock_device_satisfies_vsock_preflight() {
    classify_vsock_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
}

#[test]
fn preflight_missing_tun_module_typed() {
    let err = classify_tun_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

    assert!(matches!(err, PreflightError::TunUnavailable));
}

#[test]
fn tun_module_satisfies_tun_preflight() {
    classify_tun_availability(&HashSet::from(["tap", "bridge", "tun"]), false).unwrap();
}

#[test]
fn tun_device_satisfies_tun_preflight() {
    classify_tun_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
}

#[test]
fn preflight_missing_nf_conntrack_typed() {
    let err =
        classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

    assert!(matches!(err, PreflightError::NfConntrackUnavailable));
}

#[test]
fn nf_conntrack_module_satisfies_nat_preflight() {
    classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge", "nf_conntrack"]), false)
        .unwrap();
}

#[test]
fn nf_conntrack_sys_module_satisfies_nat_preflight() {
    classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
}

#[test]
fn preflight_missing_br_netfilter_typed() {
    let err =
        classify_br_netfilter_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

    assert!(matches!(err, PreflightError::BridgeNetfilterUnavailable));
}

#[test]
fn br_netfilter_module_satisfies_bridge_preflight() {
    classify_br_netfilter_availability(&HashSet::from(["tap", "bridge", "br_netfilter"]), false)
        .unwrap();
}

#[test]
fn br_netfilter_sys_module_satisfies_bridge_preflight() {
    classify_br_netfilter_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
}

#[test]
fn bridge_nf_call_iptables_requires_enabled_sysctl() {
    let err = classify_bridge_nf_call_iptables("0\n").unwrap_err();

    match err {
        PreflightError::BridgeNfCallIptablesDisabled { actual } => assert_eq!(actual, "0"),
        other => panic!("expected BridgeNfCallIptablesDisabled, got {other:?}"),
    }
}

#[test]
fn bridge_nf_call_iptables_accepts_enabled_sysctl() {
    classify_bridge_nf_call_iptables("1\n").unwrap();
}

#[test]
fn bridge_nf_call_iptables_rejects_unparseable_sysctl() {
    let err = classify_bridge_nf_call_iptables("not-a-number\n").unwrap_err();

    match err {
        PreflightError::BridgeNfCallIptablesDisabled { actual } => {
            assert_eq!(actual, "not-a-number");
        }
        other => panic!("expected BridgeNfCallIptablesDisabled, got {other:?}"),
    }
}

#[test]
fn nf_conntrack_capacity_requires_expected_vm_headroom() {
    let err = classify_nf_conntrack_capacity("15000\n", 8).unwrap_err();

    match err {
        PreflightError::NfConntrackCapacityTooLow {
            actual,
            minimum,
            expected_concurrent_vms,
        } => {
            assert_eq!(actual, 15_000);
            assert_eq!(minimum, 16_000);
            assert_eq!(expected_concurrent_vms, 8);
        }
        other => panic!("expected conntrack capacity failure, got {other:?}"),
    }
}

#[test]
fn nf_conntrack_capacity_accepts_expected_vm_headroom() {
    assert_eq!(
        classify_nf_conntrack_capacity("16000\n", 8).unwrap(),
        16_000
    );
}

#[test]
fn nf_conntrack_capacity_rejects_unparseable_sysctl() {
    let err = classify_nf_conntrack_capacity("not-a-number\n", 8).unwrap_err();

    assert!(matches!(err, PreflightError::InvalidNfConntrackMax { .. }));
}

#[test]
fn required_kernel_modules_report_missing_tap_bridge() {
    let err = classify_required_modules(&HashSet::from(["vhost_vsock"])).unwrap_err();

    match err {
        PreflightError::KernelModulesMissing { missing } => {
            assert_eq!(missing, vec!["tap".to_owned(), "bridge".to_owned()]);
        }
        other => panic!("expected KernelModulesMissing, got {other:?}"),
    }
}

#[test]
fn preflight_missing_kvm_returns_typed_hint() {
    let path = PathBuf::from("/dev/kvm");

    let err = classify_kvm_access(&path, false, Ok(())).unwrap_err();

    match err {
        PreflightError::KvmUnavailable { path: actual } => {
            assert_eq!(actual, path);
            assert!(
                err_hint_mentions_kvm_enable(&PreflightError::KvmUnavailable { path: actual }),
                "hint must point at enabling KVM"
            );
        }
        other => panic!("expected KvmUnavailable, got {other:?}"),
    }
}

#[test]
fn preflight_kvm_permission_denied_typed() {
    let path = PathBuf::from("/dev/kvm");

    let err = classify_kvm_access(
        &path,
        true,
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
    )
    .unwrap_err();

    match err {
        PreflightError::KvmNotWritable { path: actual } => assert_eq!(actual, path),
        other => panic!("expected KvmNotWritable, got {other:?}"),
    }
}

#[test]
fn non_access_kvm_open_errors_fail_closed() {
    // Non-EACCES open errors on /dev/kvm (EBUSY, EIO, etc.) must propagate
    // — silently passing the check on an unknown device state hides a
    // misconfiguration that will fail at boot anyway.
    let result = classify_kvm_access(
        PathBuf::from("/dev/kvm").as_path(),
        true,
        Err(io::Error::new(io::ErrorKind::Other, "busy")),
    );

    assert!(matches!(result, Err(PreflightError::PathIo { .. })));
}

#[test]
fn preflight_missing_vmx_svm_returns_typed() {
    let cpuinfo = "\
processor\t: 0
vendor_id\t: GenuineIntel
flags\t\t: fpu tsc msr pae
";

    let err = classify_kvm_cpu_flags(cpuinfo).unwrap_err();

    assert!(matches!(err, PreflightError::KvmCpuExtensionMissing));
}

#[test]
fn preflight_vmx_svm_flags_are_reported_once() {
    let cpuinfo = "\
processor\t: 0
flags\t\t: fpu vmx tsc vmx
processor\t: 1
flags\t\t: fpu svm tsc
";

    let flags = classify_kvm_cpu_flags(cpuinfo).unwrap();

    assert_eq!(flags, vec!["vmx", "svm"]);
}

#[test]
fn thp_always_policy_reports_clean_row() {
    let row = classify_thp_policy(Ok("[always] madvise never\n".to_string()));

    assert!(row.passed);
    assert_eq!(row.label, "Transparent hugepages");
    assert!(row.detail.contains("always selected"));
    assert!(!row.detail.contains("advisory:"));
}

#[test]
fn thp_madvise_policy_reports_advisory() {
    let row = classify_thp_policy(Ok("always [madvise] never\n".to_string()));

    assert!(row.passed);
    assert!(row.detail.contains("madvise selected"));
    assert!(row.detail.contains("advisory:"));
    assert!(row.detail.contains("docs/ops/host-tuning.md"));
}

#[test]
fn thp_never_policy_reports_advisory() {
    let row = classify_thp_policy(Ok("always madvise [never]\n".to_string()));

    assert!(row.passed);
    assert!(row.detail.contains("never selected"));
    assert!(row.detail.contains("advisory:"));
}

#[test]
fn thp_unreadable_policy_is_non_blocking() {
    let row = classify_thp_policy(Err(io::Error::new(
        io::ErrorKind::NotFound,
        "missing thp file",
    )));

    assert!(row.passed);
    assert!(row.detail.contains("advisory could not be evaluated"));
    assert!(row.detail.contains("docs/ops/host-tuning.md"));
}

#[test]
fn thp_malformed_policy_is_non_blocking() {
    let row = classify_thp_policy(Ok("always madvise never\n".to_string()));

    assert!(row.passed);
    assert!(row.detail.contains("unrecognized THP policy format"));
}

#[test]
fn kvm_halt_poll_reports_current_value_as_advisory() {
    let row = classify_kvm_halt_poll(Some("200000"), Some("2"), Some("2"), None, None);

    assert!(row.passed);
    assert_eq!(row.label, "KVM halt polling");
    assert!(row.detail.contains("halt_poll_ns=200000"));
    assert!(row.detail.contains("grow=2"));
    assert!(row.detail.contains("shrink=2"));
    assert!(row.detail.contains("halt_poll_ns=400000"));
    assert!(row.detail.contains("docs/ops/host-tuning.md"));
}

#[test]
fn kvm_halt_poll_reports_timer_interaction_when_available() {
    let row = classify_kvm_halt_poll(
        Some("400000"),
        Some("2"),
        Some("2"),
        Some("1000"),
        Some("Y"),
    );

    assert!(row.detail.contains("lapic_timer_advance=1000"));
    assert!(row.detail.contains("enable_preemption_timer=Y"));
}

#[test]
fn kvm_halt_poll_unavailable_is_non_blocking() {
    let row = classify_kvm_halt_poll(None, Some("2"), Some("2"), Some("1000"), Some("Y"));

    assert!(row.passed);
    assert!(row.detail.contains("halt_poll_ns=unavailable"));
    assert!(row.detail.contains("grow=2"));
    assert!(row.detail.contains("shrink=2"));
    assert!(row.detail.contains("lapic_timer_advance=1000"));
    assert!(row.detail.contains("enable_preemption_timer=Y"));
    assert!(row.detail.contains("advisory could not be evaluated"));
}

#[test]
fn cpu_governor_acpi_non_performance_reports_advisory() {
    let row = classify_cpu_governor(Some("acpi-cpufreq"), Some("ondemand"));

    assert!(row.passed);
    assert_eq!(row.label, "CPU governor");
    assert!(row.detail.contains("driver=acpi-cpufreq"));
    assert!(row.detail.contains("governor=ondemand"));
    assert!(row.detail.contains("advisory:"));
    assert!(row.detail.contains("cpupower frequency-set -g performance"));
    assert!(row.detail.contains("docs/ops/host-tuning.md"));
}

#[test]
fn cpu_governor_acpi_performance_is_clean() {
    let row = classify_cpu_governor(Some("acpi-cpufreq"), Some("performance"));

    assert!(row.passed);
    assert!(row.detail.contains("governor=performance"));
    assert!(!row.detail.contains("advisory:"));
}

#[test]
fn cpu_governor_intel_pstate_powersave_is_clean() {
    let row = classify_cpu_governor(Some("intel_pstate"), Some("powersave"));

    assert!(row.passed);
    assert!(row.detail.contains("hardware-managed pstate"));
    assert!(!row.detail.contains("advisory:"));
}

#[test]
fn cpu_governor_amd_pstate_powersave_is_clean() {
    let row = classify_cpu_governor(Some("amd_pstate"), Some("powersave"));

    assert!(row.passed);
    assert!(row.detail.contains("hardware-managed pstate"));
    assert!(!row.detail.contains("advisory:"));
}

#[test]
fn cpu_governor_unavailable_is_non_blocking() {
    let row = classify_cpu_governor(None, None);

    assert!(row.passed);
    assert!(row.detail.contains("CPU governor check not evaluated"));
    assert!(!row.detail.contains("advisory:"));
}

#[test]
fn cpu_microcode_reports_version_and_flags() {
    let row = classify_cpu_microcode(Some("0x830107c"), Some("0x0"));

    assert_eq!(row.label, "CPU microcode");
    assert!(row.passed);
    assert_eq!(row.detail, "version=0x830107c, processor_flags=0x0");
}

#[test]
fn cpu_microcode_unavailable_is_non_blocking() {
    let row = classify_cpu_microcode(None, None);

    assert!(row.passed);
    assert!(row.detail.contains("version=unavailable"));
    assert!(row.detail.contains("microcode level not reported by host"));
}

#[test]
fn cpu_vulnerability_mds_vulnerable_fails_closed() {
    let check = CpuVulnerabilityCheck {
        id: "mds",
        hard_fail_on_vulnerable: true,
    };

    let err = classify_cpu_vulnerability(
        check,
        "Vulnerable: Clear CPU buffers attempted, no microcode\n",
    )
    .expect_err("mds vulnerable status must fail");

    match err {
        PreflightError::CpuVulnerabilityDetected { id, detail } => {
            assert_eq!(id, "mds");
            assert_eq!(
                detail,
                "Vulnerable: Clear CPU buffers attempted, no microcode"
            );
        }
        other => panic!("expected CpuVulnerabilityDetected, got {other:?}"),
    }
}

#[test]
fn cpu_vulnerability_medium_vulnerable_is_advisory_row() {
    let check = CpuVulnerabilityCheck {
        id: "spectre_v2",
        hard_fail_on_vulnerable: false,
    };

    let row = classify_cpu_vulnerability(check, "Vulnerable: Retpoline without IBPB\n").unwrap();

    assert_eq!(
        row,
        "spectre_v2=vulnerable advisory: Vulnerable: Retpoline without IBPB"
    );
}

#[test]
fn cpu_vulnerability_mitigated_status_is_clean_row_detail() {
    let check = CpuVulnerabilityCheck {
        id: "retbleed",
        hard_fail_on_vulnerable: false,
    };

    let row = classify_cpu_vulnerability(check, "Mitigation: untrained return thunk\n").unwrap();

    assert_eq!(row, "retbleed=Mitigation: untrained return thunk");
}

#[test]
fn cpu_vulnerability_unclassified_status_is_advisory() {
    let check = CpuVulnerabilityCheck {
        id: "srbds",
        hard_fail_on_vulnerable: false,
    };

    let row = classify_cpu_vulnerability(check, "Unknown: vendor-specific status\n").unwrap();

    assert_eq!(
        row,
        "srbds=unclassified advisory: Unknown: vendor-specific status"
    );
}

#[test]
fn cpu_vulnerability_scan_reports_all_configured_files() {
    let dir = tempfile::tempdir().expect("create tempdir");
    for check in CPU_VULNERABILITY_CHECKS {
        fs::write(dir.path().join(check.id), "Not affected\n").expect("write status");
    }
    let mut report = Vec::new();

    check_cpu_vulnerabilities_in_dir(dir.path(), false, &mut report)
        .expect("all not affected statuses pass");

    assert_eq!(report.len(), 1);
    assert_eq!(report[0].label, "CPU vulnerabilities");
    assert!(report[0].passed);
    for check in CPU_VULNERABILITY_CHECKS {
        assert!(report[0].detail.contains(check.id), "missing {}", check.id);
    }
}

#[test]
fn cpu_vulnerability_scan_can_be_explicitly_skipped() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mut report = Vec::new();

    check_cpu_vulnerabilities_in_dir(dir.path(), true, &mut report).unwrap();

    assert_eq!(report.len(), 1);
    assert_eq!(report[0].label, "CPU vulnerabilities");
    assert_eq!(
        report[0].detail,
        "skipped by M80_SKIP_CHECK_VULNERABILITIES=1"
    );
}

fn err_hint_mentions_kvm_enable(err: &PreflightError) -> bool {
    let hint = err.hint();
    hint.contains("KVM") && hint.contains("enabled")
}
