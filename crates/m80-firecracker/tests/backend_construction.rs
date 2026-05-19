//! Backend construction and reuse behavior.

mod common;

use m80_firecracker::FcError;

#[test]
fn backend_reused_across_admissions_shares_admission_state() {
    let dir = tempfile::tempdir().unwrap();
    let backend = common::make_fake_backend(1, dir.path());

    assert_eq!(backend.config().run_root(), dir.path());
    assert_eq!(backend.config().discovery().run_root, dir.path());

    let first = backend
        .admit(common::sandbox_config())
        .expect("first admit acquires the singleton backend permit");

    let err = backend
        .admit(common::sandbox_config())
        .expect_err("same backend must share admission state across requests");
    assert!(
        matches!(err, FcError::AdmissionRefused { limit: 1 }),
        "expected shared admission refusal, got {err:?}"
    );

    drop(first);

    let second = backend
        .admit(common::sandbox_config())
        .expect("dropping the first sandbox returns the shared permit");
    drop(second);
}
