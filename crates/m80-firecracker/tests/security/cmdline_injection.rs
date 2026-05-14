use m80_firecracker::{ConfigError, FcError, NetnsSpec};

fn assert_boot_args_rejected(value: &str) {
    let run_root = tempfile::tempdir().expect("run root");
    let backend = super::common::make_fake_backend(1, run_root.path());
    let mut config = super::common::sandbox_config();
    config.boot_args = Some(value.to_owned());

    let err = backend
        .admit(config)
        .expect_err("reserved or malformed boot args must be rejected before launch");
    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::InvalidValue {
                field: "boot_args",
                ..
            })
        ),
        "expected boot_args ConfigError for {value:?}, got {err:?}"
    );
}

#[test]
fn boot_args_init_override_fails_before_admission_permit() {
    let run_root = tempfile::tempdir().expect("run root");
    let backend = super::common::make_fake_backend(1, run_root.path());
    let mut config = super::common::sandbox_config();
    config.boot_args = Some("init=/bin/sh".to_owned());

    let err = backend
        .admit(config)
        .expect_err("init override must be rejected before launch");
    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::InvalidValue {
                field: "boot_args",
                ..
            })
        ),
        "expected boot_args ConfigError, got {err:?}"
    );

    let sandbox = backend
        .admit(super::common::sandbox_config())
        .expect("rejected boot_args must not consume admission permit");
    drop(sandbox);
}

#[test]
fn boot_args_workspace_marker_injection_is_rejected() {
    assert_boot_args_rejected("m80.workspace=1");
}

#[test]
fn boot_args_rootfs_marker_injection_is_rejected() {
    assert_boot_args_rejected("m80.rootfs=erofs");
}

#[test]
fn boot_args_rootfstype_override_is_rejected() {
    assert_boot_args_rejected("rootfstype=tmpfs");
}

#[test]
fn boot_args_control_char_injection_is_rejected() {
    assert_boot_args_rejected("m80.safe=1\ninit=/bin/sh");
}

#[test]
fn join_netns_guest_mac_space_injection_is_rejected() {
    let err = m80_firecracker::MacAddr::parse("02:00:00:00:80:01 init=/bin/sh")
        .expect_err("whitespace-bearing MAC must not construct");
    assert!(
        err.to_string().contains("invalid MAC address"),
        "unexpected error: {err}"
    );
}

#[test]
fn join_netns_dns_resolver_cmdline_injection_is_rejected() {
    let err = serde_json::from_str::<NetnsSpec>(
        r#"{
            "netns_path":"/var/run/netns/m80-test",
            "tap_name":"tapm80test",
            "guest_mac":"02:00:00:00:80:01",
            "guest_ipv4":"10.80.0.2/24",
            "gateway_ipv4":"10.80.0.1",
            "dns_resolvers":["10.80.0.1 init=/bin/sh"]
        }"#,
    )
    .expect_err("DNS resolver injection must fail typed IPv4 parsing");

    assert!(
        err.to_string().contains("invalid IPv4 address syntax"),
        "unexpected error: {err}"
    );
}

#[test]
fn sandbox_config_has_no_hostname_cmdline_surface() {
    let config = super::common::sandbox_config();
    let debug = format!("{config:?}");
    assert!(
        !debug.contains("hostname"),
        "hostname-like cmdline customization must not exist outside explicit boot_args: {debug}"
    );
}
