//! Pure plan computation tests — no filesystem required.
#![allow(clippy::unwrap_used)]

mod common;

use m80_image_store::DEFAULT_STORE_ROOT;
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
    assert_eq!(first["mode"], 0o730);
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

    let steps = common::steps(&plan);
    let root = steps.first().expect("jail root step");
    assert_eq!(root["mode"], 0o730);

    for step in steps.iter().skip(1) {
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
    assert!(
        matches!(
            &err,
            JailerError::BindDestRejected { dest, .. }
                if dest == &PathBuf::from("../rootfs.ext4")
        ),
        "{err:?}"
    );
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
    assert!(
        matches!(
            &err,
            JailerError::BindDestRejected { dest, .. }
                if dest == &PathBuf::from("proc")
        ),
        "{err:?}"
    );
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
    assert!(
        matches!(
            &err,
            JailerError::BindDestRejected { dest, .. }
                if dest == &PathBuf::from("sys/devices")
        ),
        "{err:?}"
    );
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
    assert!(
        matches!(
            &err,
            JailerError::BindDestRejected { dest, .. }
                if dest == &PathBuf::from("dev/kvm")
        ),
        "{err:?}"
    );
}

#[test]
fn duplicate_bind_dest_is_rejected() {
    let mut cfg = base_config();
    cfg.bindings = vec![
        Binding {
            source: PathBuf::from("/host/kernel"),
            dest: PathBuf::from("kernel"),
            mode: BindMode::Ro,
        },
        Binding {
            source: PathBuf::from("/host/pmem"),
            dest: PathBuf::from("kernel"),
            mode: BindMode::Ro,
        },
    ];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(
        matches!(
            &err,
            JailerError::BindDestRejected { dest, .. }
                if dest == &PathBuf::from("kernel")
        ),
        "{err:?}"
    );
}

#[test]
fn pmem_dest_collisions_with_standard_assets_are_rejected() {
    for dest in [
        "kernel",
        "rootfs.ext4",
        "rootfs.overlay.ext4",
        "scratch.ext4",
        "hotplug-slot-0.raw",
    ] {
        let mut cfg = base_config();
        cfg.bindings = vec![
            Binding {
                source: PathBuf::from("/host/asset"),
                dest: PathBuf::from(dest),
                mode: BindMode::Ro,
            },
            Binding {
                source: PathBuf::from("/host/pmem.0.img"),
                dest: PathBuf::from(dest),
                mode: BindMode::Ro,
            },
        ];

        let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
        assert!(
            matches!(
                &err,
                JailerError::BindDestRejected { dest: rejected, .. }
                    if rejected == &PathBuf::from(dest)
            ),
            "{dest}: {err:?}"
        );
    }
}

#[test]
fn pmem_bind_dest_at_jail_root_is_accepted() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/host/pmem.0.img"),
        dest: PathBuf::from("pmem.0.img"),
        mode: BindMode::Ro,
    }];

    let plan = m80_jailer::Plan::compute(&cfg).unwrap();
    let bind_steps = common::bind_steps(&plan);

    assert_eq!(bind_steps.len(), 1);
    assert_eq!(
        bind_steps[0].1,
        PathBuf::from("/tmp/run/vm-1/firecracker/vm-1/root/pmem.0.img")
    );
    assert_eq!(bind_steps[0].2, BindMode::Ro);
}

#[test]
fn shared_image_store_bind_source_outside_default_store_is_rejected() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/tmp/not-m80-images/image.erofs"),
        dest: PathBuf::from("pmem.0.img"),
        mode: BindMode::RoImageStore,
    }];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(
        matches!(
            &err,
            JailerError::BindSourceRejected { src, expected_root }
                if src == &PathBuf::from("/tmp/not-m80-images/image.erofs")
                    && expected_root == &PathBuf::from(DEFAULT_STORE_ROOT)
        ),
        "{err:?}"
    );
}

#[test]
fn shared_image_store_bind_keeps_same_source_while_per_vm_paths_differ() {
    let shared_source = PathBuf::from(DEFAULT_STORE_ROOT)
        .join("58")
        .join("589fb7cdcec07171aa6892a7c86870bac98494d4e0734154f24ae4fb9ea9cf37")
        .join("image.erofs");
    let mut first_shared = base_config();
    first_shared.run_dir = PathBuf::from("/tmp/run/vm-shared-a");
    first_shared.bindings = vec![Binding {
        source: shared_source.clone(),
        dest: PathBuf::from("pmem.0.img"),
        mode: BindMode::RoImageStore,
    }];
    let mut second_shared = base_config();
    second_shared.run_dir = PathBuf::from("/tmp/run/vm-shared-b");
    second_shared.bindings = vec![Binding {
        source: shared_source.clone(),
        dest: PathBuf::from("pmem.0.img"),
        mode: BindMode::RoImageStore,
    }];

    let first_shared_plan = m80_jailer::Plan::compute(&first_shared).unwrap();
    let second_shared_plan = m80_jailer::Plan::compute(&second_shared).unwrap();
    let first_shared_bind = common::bind_steps(&first_shared_plan);
    let second_shared_bind = common::bind_steps(&second_shared_plan);

    assert_eq!(first_shared_bind[0].0, shared_source);
    assert_eq!(second_shared_bind[0].0, first_shared_bind[0].0);
    assert_eq!(first_shared_bind[0].2, BindMode::RoImageStore);
    assert_eq!(second_shared_bind[0].2, BindMode::RoImageStore);

    let mut first_per_vm = base_config();
    first_per_vm.run_dir = PathBuf::from("/tmp/run/vm-per-vm-a");
    first_per_vm.bindings = vec![Binding {
        source: PathBuf::from("/tmp/run/vm-per-vm-a/pmem/0.img"),
        dest: PathBuf::from("pmem.0.img"),
        mode: BindMode::Ro,
    }];
    let mut second_per_vm = base_config();
    second_per_vm.run_dir = PathBuf::from("/tmp/run/vm-per-vm-b");
    second_per_vm.bindings = vec![Binding {
        source: PathBuf::from("/tmp/run/vm-per-vm-b/pmem/0.img"),
        dest: PathBuf::from("pmem.0.img"),
        mode: BindMode::Ro,
    }];

    let first_per_vm_bind = common::bind_steps(&m80_jailer::Plan::compute(&first_per_vm).unwrap());
    let second_per_vm_bind =
        common::bind_steps(&m80_jailer::Plan::compute(&second_per_vm).unwrap());

    assert_ne!(first_per_vm_bind[0].0, second_per_vm_bind[0].0);
    assert_eq!(first_per_vm_bind[0].2, BindMode::Ro);
    assert_eq!(second_per_vm_bind[0].2, BindMode::Ro);
}

#[test]
fn pmem_bind_dest_cannot_escape_with_parent_component() {
    let mut cfg = base_config();
    cfg.bindings = vec![Binding {
        source: PathBuf::from("/host/pmem.0.img"),
        dest: PathBuf::from("../pmem.0.img"),
        mode: BindMode::Ro,
    }];

    let err = m80_jailer::Plan::compute(&cfg).unwrap_err();
    assert!(
        matches!(
            &err,
            JailerError::BindDestRejected { dest, .. }
                if dest == &PathBuf::from("../pmem.0.img")
        ),
        "{err:?}"
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
