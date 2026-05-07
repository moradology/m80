use std::path::{Path, PathBuf};

use m80_jailer::{BindMode, Binding, JailerConfig, JailerSocket, Plan};

fn config() -> JailerConfig {
    crate::common::minimal_config(Path::new("/tmp/run/vm-asset"))
}

#[test]
fn binds_kernel_and_rootfs_ro() {
    let mut cfg = config();
    cfg.bindings = vec![
        Binding {
            source: PathBuf::from("/host/vmlinux"),
            dest: PathBuf::from("kernel"),
            mode: BindMode::Ro,
        },
        Binding {
            source: PathBuf::from("/host/rootfs.ext4"),
            dest: PathBuf::from("rootfs.ext4"),
            mode: BindMode::Ro,
        },
    ];

    let plan = Plan::compute(&cfg).unwrap();
    let binds = crate::common::bind_steps(&plan);

    assert!(binds.iter().any(|(source, dest, mode)| {
        *source == Path::new("/host/vmlinux")
            && dest.ends_with("root/kernel")
            && *mode == BindMode::Ro
    }));
    assert!(binds.iter().any(|(source, dest, mode)| {
        *source == Path::new("/host/rootfs.ext4")
            && dest.ends_with("root/rootfs.ext4")
            && *mode == BindMode::Ro
    }));
}

#[test]
fn binds_drives_rw() {
    let mut cfg = config();
    cfg.bindings = vec![
        Binding {
            source: PathBuf::from("/run/vm/rootfs.overlay.ext4"),
            dest: PathBuf::from("rootfs.overlay.ext4"),
            mode: BindMode::Rw,
        },
        Binding {
            source: PathBuf::from("/run/vm/scratch.ext4"),
            dest: PathBuf::from("scratch.ext4"),
            mode: BindMode::Rw,
        },
    ];

    let plan = Plan::compute(&cfg).unwrap();
    let binds = crate::common::bind_steps(&plan);

    assert!(binds.iter().any(|(source, dest, mode)| {
        *source == Path::new("/run/vm/rootfs.overlay.ext4")
            && dest.ends_with("root/rootfs.overlay.ext4")
            && *mode == BindMode::Rw
    }));
    assert!(binds.iter().any(|(source, dest, mode)| {
        *source == Path::new("/run/vm/scratch.ext4")
            && dest.ends_with("root/scratch.ext4")
            && *mode == BindMode::Rw
    }));
}

#[test]
fn sockets_created_inside_jail() {
    let mut cfg = config();
    cfg.sockets = vec![JailerSocket::Firecracker, JailerSocket::Vsock];

    let plan = Plan::compute(&cfg).unwrap();
    let sockets = crate::common::socket_steps(&plan);

    assert_eq!(sockets.len(), 2);
    assert!(sockets
        .iter()
        .any(|path| path.ends_with("root/firecracker.sock")));
    assert!(sockets.iter().any(|path| path.ends_with("root/vsock.sock")));
}

#[test]
fn host_only_artifacts_listed() {
    let mut cfg = config();
    cfg.stdio_log = Some(PathBuf::from("/run/vm/console.log"));
    cfg.sockets = vec![JailerSocket::Firecracker];

    let plan = Plan::compute(&cfg).unwrap();
    let serialized = serde_json::to_string(&plan.steps).unwrap();

    for host_only in [
        "ownership.lock",
        "jailer-plan.json",
        "jailer-state.json",
        "boot.identity.json",
        "console.log",
        "diagnostics.log",
        "metrics.json",
    ] {
        assert!(
            !serialized.contains(host_only),
            "{host_only} must not become a jail materialization step"
        );
    }
}
