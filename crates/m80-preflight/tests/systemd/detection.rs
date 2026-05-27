use m80_preflight::{HostPrerequisiteCheckId, SYSTEMD_MIN_VERSION};

#[test]
fn systemd_check_id_is_in_the_stable_registry() {
    assert!(
        HostPrerequisiteCheckId::ALL.contains(&HostPrerequisiteCheckId::Systemd),
        "Systemd check id must be part of the full-report registry"
    );
    assert_eq!(HostPrerequisiteCheckId::Systemd.as_str(), "systemd");
    assert_eq!(
        HostPrerequisiteCheckId::Systemd.check_name(),
        "systemd launch"
    );
}

#[test]
fn systemd_minimum_version_is_pinned_for_detection() {
    assert_eq!(SYSTEMD_MIN_VERSION, 245);
}
