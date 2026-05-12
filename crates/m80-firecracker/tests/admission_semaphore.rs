//! Admission semaphore: permit counting, refusal, and permit return on drop.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::Barrier;

use m80_firecracker::{FcError, Sandbox, SandboxConfig};

fn make_backend(max: u32) -> std::sync::Arc<m80_firecracker::Backend> {
    common::make_fake_backend(max as usize, std::path::Path::new("/tmp/m80-test"))
}

#[test]
fn admit_up_to_limit_succeeds() {
    let backend = make_backend(2);
    let s1 = backend
        .admit(common::sandbox_config())
        .expect("first admit should succeed");
    let s2 = backend
        .admit(common::sandbox_config())
        .expect("second admit should succeed");
    drop(s1);
    drop(s2);
}

#[test]
fn admit_beyond_limit_returns_refused() {
    let backend = make_backend(2);
    let s1 = backend
        .admit(common::sandbox_config())
        .expect("first admit should succeed");
    let s2 = backend
        .admit(common::sandbox_config())
        .expect("second admit should succeed");

    let err = backend
        .admit(common::sandbox_config())
        .expect_err("third admit should be refused");

    assert!(
        matches!(err, FcError::AdmissionRefused { limit: 2 }),
        "expected AdmissionRefused(limit=2), got {err:?}"
    );

    drop(s1);
    drop(s2);
}

#[test]
fn admission_semaphore_concurrent_overflow() {
    let limit = 4;
    let contenders = limit + 1;
    let backend = make_backend(limit as u32);
    let start = Arc::new(Barrier::new(contenders));
    let release = Arc::new(Barrier::new(contenders + 1));
    let (tx, rx) = std::sync::mpsc::channel();

    let handles = (0..contenders)
        .map(|idx| {
            let backend = Arc::clone(&backend);
            let start = Arc::clone(&start);
            let release = Arc::clone(&release);
            let tx = tx.clone();
            std::thread::spawn(move || {
                start.wait();
                let admit = backend.admit(SandboxConfig {
                    vm_id: Some(format!("concurrent-admit-{idx}")),
                    ..common::sandbox_config()
                });
                let outcome = match &admit {
                    Ok(_) => AdmitOutcome::Accepted,
                    Err(FcError::AdmissionRefused { limit }) => AdmitOutcome::Refused(*limit),
                    Err(_) => AdmitOutcome::Unexpected,
                };
                tx.send(outcome).expect("send admit outcome");
                release.wait();
                drop(admit);
            })
        })
        .collect::<Vec<_>>();
    drop(tx);

    let outcomes = rx.iter().take(contenders).collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AdmitOutcome::Accepted))
            .count(),
        limit
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AdmitOutcome::Refused(4)))
            .count(),
        1,
        "outcomes: {outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .all(|outcome| !matches!(outcome, AdmitOutcome::Unexpected)),
        "outcomes: {outcomes:?}"
    );

    release.wait();
    for handle in handles {
        handle.join().expect("admission contender thread");
    }

    let retry = backend
        .admit(common::sandbox_config())
        .expect("slot must return after accepted contenders drop");
    drop(retry);
}

#[test]
fn permit_drop_restores_slot() {
    let backend = make_backend(1);
    let s1 = backend
        .admit(common::sandbox_config())
        .expect("first admit should succeed");

    // At capacity — second admit fails.
    backend
        .admit(common::sandbox_config())
        .expect_err("should be refused at limit");

    // Dropping s1 returns the permit; next admit should succeed.
    drop(s1);

    let s2 = backend
        .admit(common::sandbox_config())
        .expect("admit after drop should succeed");
    drop(s2);
}

#[test]
fn failed_launch_returns_admission_slot() {
    let dir = tempfile::tempdir().unwrap();
    let backend = common::make_fake_backend(1, dir.path());
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some("failed-launch-slot".to_string()),
            overlay_size_bytes: 64 * 1024 * 1024,
            ..SandboxConfig::default()
        })
        .expect("first admit must acquire the only slot");

    let err = sandbox.launch().expect_err("fake backend launch must fail");
    assert!(
        !err.to_string().is_empty(),
        "launch failure must be a typed displayable error"
    );

    let second = backend.admit(SandboxConfig {
        vm_id: Some("second-after-failure".to_string()),
        ..SandboxConfig::default()
    });
    assert!(
        second.is_ok(),
        "failed launch must drop the admission permit and free the slot"
    );
}

#[test]
fn launch_panic_releases_permit() {
    let backend = make_backend(1);
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some("panic-release".to_string()),
            ..common::sandbox_config()
        })
        .expect("first admit must acquire the only slot");

    let panic_result = catch_unwind(AssertUnwindSafe(|| injected_launch_panic(sandbox)));

    assert!(panic_result.is_err(), "panic injection must unwind");
    let retry = backend.admit(SandboxConfig {
        vm_id: Some("after-panic".to_string()),
        ..common::sandbox_config()
    });
    assert!(
        retry.is_ok(),
        "panic while launch owns the sandbox must release the admission permit"
    );
}

#[derive(Debug)]
enum AdmitOutcome {
    Accepted,
    Refused(u32),
    Unexpected,
}

fn injected_launch_panic(_sandbox: Sandbox) {
    panic!("injected launch panic");
}
