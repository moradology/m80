//! m80-guestd systemd unit console wiring tests.

#[test]
fn guestd_service_routes_stdout_and_stderr_to_journal_and_console() {
    let unit = include_str!("../assets/m80-guestd.service");
    assert!(unit.contains("StandardOutput=journal+console"), "{unit}");
    assert!(unit.contains("StandardError=journal+console"), "{unit}");
}
