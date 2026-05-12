use super::*;

#[test]
fn cgroup_mode_parser_accepts_disabled() {
    let mode = parse_cgroup_mode("disabled").unwrap();

    assert_eq!(mode, CgroupPreflightMode::Disabled);
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
fn preflight_missing_vsock_module_typed() {
    let err =
        classify_vsock_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

    assert!(matches!(err, PreflightError::VsockUnavailable));
}

#[test]
fn vhost_vsock_module_satisfies_vsock_preflight() {
    classify_vsock_availability(&HashSet::from(["tap", "bridge", "vhost_vsock"]), false)
        .unwrap();
}

#[test]
fn vhost_vsock_device_satisfies_vsock_preflight() {
    classify_vsock_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
}

#[test]
fn preflight_missing_tun_module_typed() {
    let err =
        classify_tun_availability(&HashSet::from(["tap", "bridge"]), false).unwrap_err();

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
        classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge"]), false)
            .unwrap_err();

    assert!(matches!(err, PreflightError::NfConntrackUnavailable));
}

#[test]
fn nf_conntrack_module_satisfies_nat_preflight() {
    classify_nf_conntrack_availability(
        &HashSet::from(["tap", "bridge", "nf_conntrack"]),
        false,
    )
    .unwrap();
}

#[test]
fn nf_conntrack_sys_module_satisfies_nat_preflight() {
    classify_nf_conntrack_availability(&HashSet::from(["tap", "bridge"]), true).unwrap();
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

fn err_hint_mentions_kvm_enable(err: &PreflightError) -> bool {
    let hint = err.hint();
    hint.contains("KVM") && hint.contains("enabled")
}
