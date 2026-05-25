mod common;

use m80_firecracker::{ConfigError, FcError, SandboxConfig};

#[test]
fn admit_accepts_2m_hugepages_for_aligned_memory() {
    let run_root = tempfile::tempdir().expect("run root");
    let backend = common::make_fake_backend(1, run_root.path());
    let config = SandboxConfig {
        huge_pages_2m: true,
        mem_size_mib: Some(512),
        ..common::sandbox_config()
    };

    backend.admit(config).expect("aligned hugepage config");
}

#[test]
fn admit_rejects_2m_hugepages_for_odd_memory() {
    let run_root = tempfile::tempdir().expect("run root");
    let backend = common::make_fake_backend(1, run_root.path());
    let config = SandboxConfig {
        huge_pages_2m: true,
        mem_size_mib: Some(513),
        ..common::sandbox_config()
    };

    let err = backend
        .admit(config)
        .expect_err("odd MiB memory cannot be backed by 2 MiB hugepages");

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::InvalidValue {
                field: "huge_pages_2m",
                ..
            })
        ),
        "expected huge_pages_2m config rejection, got {err:?}"
    );
}
