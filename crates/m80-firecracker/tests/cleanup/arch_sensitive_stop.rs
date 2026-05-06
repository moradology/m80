use m80_firecracker::{StopDisposition, STOP_DISPOSITIONS};

#[test]
fn current_stop_path_is_arch_independent_guestd_shutdown_then_kill() {
    assert_eq!(
        STOP_DISPOSITIONS[0],
        StopDisposition::GuestdShutdownThenFirecrackerKill
    );
}

#[test]
fn force_kill_is_explicit_host_kill_path() {
    assert!(STOP_DISPOSITIONS.contains(&StopDisposition::HostForceKill));
}

#[test]
fn disposition_set_has_no_unsupported_arch_special_case() {
    assert_eq!(
        STOP_DISPOSITIONS,
        [
            StopDisposition::GuestdShutdownThenFirecrackerKill,
            StopDisposition::HostForceKill,
        ]
    );
}
