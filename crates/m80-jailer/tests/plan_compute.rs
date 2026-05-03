//! Pure plan computation tests — no filesystem required.

use m80_jailer::{BindMode, Binding, JailerConfig, JailerError, PlanStep, SocketSpec};
use std::path::PathBuf;

fn base_config() -> JailerConfig {
    JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: PathBuf::from("/tmp/run/vm-1"),
        uid: 3000,
        gid: 3000,
        bindings: Vec::new(),
        sockets: Vec::new(),
    }
}

#[test]
fn uid_zero_rejected() {
    let mut cfg = base_config();
    cfg.uid = 0;
    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(
        matches!(err, JailerError::UidGidInvalid { uid: 0, gid: 3000 }),
        "expected UidGidInvalid for uid=0, got {err:?}"
    );
}

#[test]
fn gid_zero_rejected() {
    let mut cfg = base_config();
    cfg.gid = 0;
    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(
        matches!(err, JailerError::UidGidInvalid { uid: 3000, gid: 0 }),
        "expected UidGidInvalid for gid=0, got {err:?}"
    );
}

#[test]
fn both_uid_gid_zero_rejected() {
    let mut cfg = base_config();
    cfg.uid = 0;
    cfg.gid = 0;
    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(
        matches!(err, JailerError::UidGidInvalid { uid: 0, gid: 0 }),
        "expected UidGidInvalid for uid=0 gid=0, got {err:?}"
    );
}

#[test]
fn determinism_byte_equal_json() {
    let mut cfg = base_config();
    cfg.bindings = vec![
        Binding {
            source: PathBuf::from("/host/kernel"),
            dest: PathBuf::from("kernel/vmlinux"),
            mode: BindMode::Ro,
        },
        Binding {
            source: PathBuf::from("/host/rootfs.ext4"),
            dest: PathBuf::from("drives/rootfs.ext4"),
            mode: BindMode::Rw,
        },
    ];
    cfg.sockets = vec![SocketSpec {
        path: PathBuf::from("run/firecracker.sock"),
    }];

    let plan_a = m80_jailer::Plan::compute(&cfg).unwrap();
    let plan_b = m80_jailer::Plan::compute(&cfg).unwrap();

    let json_a = serde_json::to_vec(&plan_a).unwrap();
    let json_b = serde_json::to_vec(&plan_b).unwrap();
    assert_eq!(json_a, json_b, "two Plan::compute calls must be byte-equal");
}

#[test]
fn step_ordering_jail_root_first() {
    let cfg = base_config();
    let plan = m80_jailer::Plan::compute(&cfg).unwrap();
    let first = plan.steps.first().expect("plan must have at least one step");
    // Jailer's hardcoded chroot layout: <run_dir>/<exec basename>/<id>/root/.
    // With `firecracker_bin = /usr/bin/firecracker` (basename `firecracker`)
    // and `run_dir = /tmp/run/vm-1` (basename used as `--id`), the chroot
    // is at /tmp/run/vm-1/firecracker/vm-1/root.
    assert!(
        matches!(first, PlanStep::CreateDir { path, .. } if path == &PathBuf::from("/tmp/run/vm-1/firecracker/vm-1/root")),
        "first step must be CreateDir for jail root, got {first:?}"
    );
}

#[test]
fn step_ordering_create_inside_before_binds() {
    let mut cfg = base_config();
    cfg.bindings = vec![
        Binding {
            source: PathBuf::from("/host/kernel"),
            dest: PathBuf::from("kernel/vmlinux"),
            mode: BindMode::Ro,
        },
        Binding {
            source: PathBuf::from(""),
            dest: PathBuf::from("run"),
            mode: BindMode::CreateInsideJail,
        },
    ];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();

    // CreateDir steps must appear before Bind steps (after jail root).
    let mut saw_bind = false;
    for step in &plan.steps {
        match step {
            PlanStep::CreateDir { .. } => {
                assert!(
                    !saw_bind,
                    "CreateDir appeared after a Bind step: {plan:?}"
                );
            }
            PlanStep::Bind { .. } => {
                saw_bind = true;
            }
            PlanStep::Socket { .. } => {}
        }
    }
}

#[test]
fn step_ordering_sockets_last() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/host/kernel"),
        dest: PathBuf::from("kernel/vmlinux"),
        mode: BindMode::Ro,
    }];
    cfg.sockets = vec![SocketSpec {
        path: PathBuf::from("run/fc.sock"),
    }];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();

    let last = plan.steps.last().expect("plan must have steps");
    assert!(
        matches!(last, PlanStep::Socket { .. }),
        "last step must be Socket, got {last:?}"
    );
}

#[test]
fn declared_order_preserved_for_binds() {
    let mut cfg = base_config();
    cfg.bindings = vec![
        Binding {
            source: PathBuf::from("/host/a"),
            dest: PathBuf::from("a"),
            mode: BindMode::Ro,
        },
        Binding {
            source: PathBuf::from("/host/b"),
            dest: PathBuf::from("b"),
            mode: BindMode::Rw,
        },
    ];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();
    let bind_steps: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|s| {
            if let PlanStep::Bind { source, .. } = s {
                Some(source.clone())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        bind_steps,
        vec![PathBuf::from("/host/a"), PathBuf::from("/host/b")],
        "bind steps must preserve declared order"
    );
}
