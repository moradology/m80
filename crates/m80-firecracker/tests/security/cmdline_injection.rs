use m80_firecracker::{ConfigError, FcError};

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
fn join_netns_guest_mac_space_injection_is_rejected() {
    let err = m80_firecracker::MacAddr::parse("02:00:00:00:80:01 init=/bin/sh")
        .expect_err("whitespace-bearing MAC must not construct");
    assert!(
        err.to_string().contains("invalid MAC address"),
        "unexpected error: {err}"
    );
}
