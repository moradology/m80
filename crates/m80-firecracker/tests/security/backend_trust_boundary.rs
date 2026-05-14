use m80_firecracker::{ConfigError, FcError};

#[test]
fn caller_vm_id_shape_fails_before_admission_permit() {
    let run_root = tempfile::tempdir().expect("run root");
    let backend = super::common::make_fake_backend(1, run_root.path());

    for vm_id in [
        "",
        "bad space",
        "bad/slash",
        "bad\\slash",
        "bad:colon",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let mut config = super::common::sandbox_config();
        config.vm_id = Some(vm_id.to_owned());
        let err = backend
            .admit(config)
            .expect_err("invalid vm_id must be rejected before launch");
        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::InvalidValue { field: "vm_id", .. })
            ),
            "expected vm_id ConfigError for {vm_id:?}, got {err:?}"
        );
    }

    let sandbox = backend
        .admit(super::common::sandbox_config_with_id("valid-vm_1.2"))
        .expect("invalid vm_id attempts must not consume the admission permit");
    drop(sandbox);
}

#[test]
fn startup_recovery_preserves_invalid_run_root_child_names() {
    let run_root = tempfile::tempdir().expect("run root");
    let invalid = run_root.path().join("bad name");
    std::fs::create_dir(&invalid).expect("invalid run dir");

    let _backend = super::common::make_fake_backend(1, run_root.path());

    assert!(
        invalid.exists(),
        "stale recovery must not treat invalid child names as vm ids"
    );
}
