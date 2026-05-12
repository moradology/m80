//! Pure plan computation tests — no filesystem required.

mod common;

use m80_jailer::{BindMode, Binding, JailerConfig, JailerError, JailerSocket};
use std::path::{Path, PathBuf};

fn base_config() -> JailerConfig {
    common::minimal_config(Path::new("/tmp/run/vm-1"))
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
    cfg.sockets = vec![JailerSocket::Firecracker];

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
    let steps = common::steps(&plan);
    let first = steps.first().expect("plan must have at least one step");
    // Jailer's hardcoded chroot layout: <run_dir>/<exec basename>/<id>/root/.
    // With `firecracker_bin = /usr/bin/firecracker` (basename `firecracker`)
    // and `run_dir = /tmp/run/vm-1` (basename used as `--id`), the chroot
    // is at /tmp/run/vm-1/firecracker/vm-1/root.
    assert!(
        first["kind"] == "create_dir" && first["path"] == "/tmp/run/vm-1/firecracker/vm-1/root",
        "first step must be CreateDir for jail root, got {first:?}"
    );
}

#[test]
fn jail_internal_dirs_are_private() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from(""),
        dest: PathBuf::from("run"),
        mode: BindMode::CreateInsideJail,
    }];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();

    for step in common::steps(&plan) {
        if step["kind"] == "create_dir" {
            assert_eq!(step["mode"], 0o700);
        }
    }
}

#[test]
fn dest_with_parent_component_is_rejected() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/host/rootfs.ext4"),
        dest: PathBuf::from("../rootfs.ext4"),
        mode: BindMode::Ro,
    }];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(matches!(err, JailerError::BindFailed { .. }), "{err:?}");
}

#[test]
fn proc_dest_is_rejected() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/proc"),
        dest: PathBuf::from("proc"),
        mode: BindMode::Ro,
    }];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(matches!(err, JailerError::BindFailed { .. }), "{err:?}");
}

#[test]
fn sys_dest_is_rejected() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/sys"),
        dest: PathBuf::from("sys/devices"),
        mode: BindMode::Ro,
    }];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(matches!(err, JailerError::BindFailed { .. }), "{err:?}");
}

#[test]
fn dev_dest_is_rejected() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/dev/kvm"),
        dest: PathBuf::from("dev/kvm"),
        mode: BindMode::Ro,
    }];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(matches!(err, JailerError::BindFailed { .. }), "{err:?}");
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
    for step in common::steps(&plan) {
        match step["kind"].as_str().expect("step kind") {
            "create_dir" => {
                assert!(!saw_bind, "CreateDir appeared after a Bind step: {plan:?}");
            }
            "bind" => {
                saw_bind = true;
            }
            "socket" => {}
            other => panic!("unexpected step kind {other}"),
        }
    }
}

#[test]
fn bind_parent_directories_are_created_before_binds() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/host/kernel"),
        dest: PathBuf::from("kernel/vmlinux"),
        mode: BindMode::Ro,
    }];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();
    let parent = PathBuf::from("/tmp/run/vm-1/firecracker/vm-1/root/kernel");
    let steps = common::steps(&plan);
    let bind_index = steps
        .iter()
        .position(|step| step["kind"] == "bind")
        .expect("bind step");
    let parent_index = steps
        .iter()
        .position(|step| {
            step["kind"] == "create_dir" && step["path"].as_str() == Some(parent.to_str().unwrap())
        })
        .expect("parent dir step");

    assert!(parent_index < bind_index, "{plan:?}");
}

#[test]
fn step_ordering_sockets_last() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/host/kernel"),
        dest: PathBuf::from("kernel/vmlinux"),
        mode: BindMode::Ro,
    }];
    cfg.sockets = vec![JailerSocket::Firecracker];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();

    let steps = common::steps(&plan);
    let last = steps.last().expect("plan must have steps");
    assert!(
        last["kind"] == "socket",
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
    let bind_steps: Vec<_> = common::bind_steps(&plan)
        .into_iter()
        .map(|(src, _, _)| src)
        .collect();

    assert_eq!(
        bind_steps,
        vec![PathBuf::from("/host/a"), PathBuf::from("/host/b")],
        "bind steps must preserve declared order"
    );
}
