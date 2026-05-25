//! Each `FcError` variant renders via `Display` without panic.

use m80_firecracker::{ConfigError, FcError, FcErrorKind, HostFaultKind};

#[test]
fn admission_refused_displays() {
    let e = FcError::AdmissionRefused { limit: 8 };
    let s = e.to_string();
    assert!(s.contains("8"), "expected limit in message, got: {s}");
}

#[test]
fn invalid_state_displays() {
    let e = FcError::InvalidState {
        expected: "Running",
        actual: "Stopped",
    };
    let s = e.to_string();
    assert!(
        s.contains("Running"),
        "expected 'Running' in message, got: {s}"
    );
    assert!(
        s.contains("Stopped"),
        "expected 'Stopped' in message, got: {s}"
    );
}

#[test]
fn api_socket_timeout_is_typed() {
    let e = FcError::ApiSocketTimeout {
        path: "/run/m80/vm/firecracker.sock".into(),
        timeout: std::time::Duration::from_secs(5),
    };
    let s = e.to_string();
    assert!(s.contains("firecracker.sock"), "got: {s}");
    assert!(s.contains("5s"), "got: {s}");
}

#[test]
fn guestd_ready_timeout_is_typed() {
    let e = FcError::GuestdReadyTimeout {
        path: "/run/m80/vm/vsock.sock_9000".into(),
        timeout: std::time::Duration::from_secs(60),
    };
    let s = e.to_string();
    assert!(s.contains("vsock.sock_9000"), "got: {s}");
    assert!(s.contains("60s"), "got: {s}");
}

#[test]
fn host_infrastructure_is_typed() {
    let e = FcError::HostInfrastructure {
        kind: HostFaultKind::ApiSocketTimeout,
        detail: "firecracker.sock did not appear".into(),
    };
    let s = e.to_string();
    assert!(s.contains("ApiSocketTimeout"), "got: {s}");
    assert!(s.contains("firecracker.sock"), "got: {s}");
    assert_eq!(e.variant_name(), "HostInfrastructure");
    assert_eq!(e.kind(), FcErrorKind::Transient);
    assert!(e.is_retryable());
}

#[test]
fn one_shot_consumed_displays() {
    let e = FcError::OneShotConsumed;
    let s = e.to_string();
    assert!(s.contains("one-shot"), "got: {s}");
}

#[test]
fn lifecycle_failure_kinds_are_bounded() {
    use m80_firecracker::LifecycleFailureKind;

    assert_eq!(LifecycleFailureKind::ALL.len(), 7);
    assert_eq!(
        LifecycleFailureKind::ALL,
        [
            LifecycleFailureKind::GuestdNotReady,
            LifecycleFailureKind::BrokenVsock,
            LifecycleFailureKind::StuckVm,
            LifecycleFailureKind::GracefulStopTimeout,
            LifecycleFailureKind::ForcedKillFallback,
            LifecycleFailureKind::CleanupFailure,
            LifecycleFailureKind::WritebackSkippedAfterUncleanStop,
        ]
    );
}

#[test]
fn file_upload_read_failed_displays() {
    let e = FcError::FileUploadReadFailed {
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
    };
    let s = e.to_string();
    assert!(
        s.contains("no such file"),
        "expected source message in display, got: {s}"
    );
    assert_eq!(e.kind(), FcErrorKind::UserInput);
    assert!(e.is_user_error());
}

#[test]
fn host_io_preserves_operation_label() {
    let e = FcError::HostIo {
        operation: "write stdout",
        source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"),
    };
    let s = e.to_string();
    assert!(s.contains("write stdout"), "got: {s}");
    assert!(s.contains("closed"), "got: {s}");
    assert_eq!(e.variant_name(), "HostIo");
    assert_eq!(e.kind(), FcErrorKind::Transient);
}

#[test]
fn config_error_displays() {
    let e = FcError::Config(ConfigError::InvalidValue {
        field: "test",
        reason: "test config error".into(),
    });
    let s = e.to_string();
    assert!(s.contains("test config error"), "got: {s}");
}

#[test]
fn shared_pmem_compressed_erofs_is_user_input() {
    let e = FcError::Config(ConfigError::SharedPmemCompressedErofs {
        path: "/store/sha256/toolchain.erofs".into(),
        compressed_files: 7,
    });
    let s = e.to_string();

    assert!(s.contains("toolchain.erofs"), "got: {s}");
    assert!(s.contains("7 compressed"), "got: {s}");
    assert_eq!(e.kind(), FcErrorKind::UserInput);
    assert!(!e.is_retryable());
    assert!(e.is_user_error());
}

#[test]
fn pmem_image_too_large_is_user_input() {
    let e = FcError::Config(ConfigError::PmemImageTooLarge {
        path: "/store/sha256/toolchain.erofs".into(),
        got: 549_755_813_889,
        max: 549_755_813_888,
    });
    let s = e.to_string();

    assert!(s.contains("toolchain.erofs"), "got: {s}");
    assert!(s.contains("549755813889"), "got: {s}");
    assert!(s.contains("549755813888"), "got: {s}");
    assert_eq!(e.kind(), FcErrorKind::UserInput);
    assert!(!e.is_retryable());
    assert!(e.is_user_error());
}

#[test]
fn invalid_vm_id_displays_rejected_value() {
    let e = FcError::InvalidVmId {
        vm_id: "..".into(),
        reason: "must not be a traversal component".into(),
    };
    let s = e.to_string();
    assert!(s.contains(".."), "got: {s}");
    assert!(
        s.contains("traversal component"),
        "expected rejection reason in display, got: {s}"
    );
}

#[test]
fn admission_refused_is_retryable_resource_exhaustion() {
    let e = FcError::AdmissionRefused { limit: 1 };

    assert_eq!(e.kind(), FcErrorKind::ResourceExhaustion);
    assert!(e.is_retryable());
    assert!(!e.is_user_error());
}

#[test]
fn invalid_vm_id_is_user_input() {
    let e = FcError::InvalidVmId {
        vm_id: "/tmp/nope".into(),
        reason: "must be a single safe path component".into(),
    };

    assert_eq!(e.kind(), FcErrorKind::UserInput);
    assert!(!e.is_retryable());
    assert!(e.is_user_error());
}

#[test]
fn api_socket_timeout_is_retryable_transient() {
    let e = FcError::ApiSocketTimeout {
        path: "/run/m80/vm/firecracker.sock".into(),
        timeout: std::time::Duration::from_secs(5),
    };

    assert_eq!(e.kind(), FcErrorKind::Transient);
    assert!(e.is_retryable());
    assert!(!e.is_user_error());
}

#[test]
fn guestd_ready_timeout_is_guest_outcome_not_retryable() {
    let e = FcError::GuestdReadyTimeout {
        path: "/run/m80/vm/vsock.sock_9000".into(),
        timeout: std::time::Duration::from_secs(60),
    };

    assert_eq!(e.kind(), FcErrorKind::GuestOutcome);
    assert!(!e.is_retryable());
    assert!(!e.is_user_error());
}

#[test]
fn idle_timeout_is_guest_outcome_not_retryable() {
    let e = FcError::IdleTimedOut;

    assert_eq!(e.kind(), FcErrorKind::GuestOutcome);
    assert!(!e.is_retryable());
    assert!(!e.is_user_error());
}

#[test]
fn invalid_state_is_internal_not_retryable() {
    let e = FcError::InvalidState {
        expected: "Running",
        actual: "Stopped",
    };

    assert_eq!(e.kind(), FcErrorKind::Internal);
    assert!(!e.is_retryable());
    assert!(!e.is_user_error());
}
