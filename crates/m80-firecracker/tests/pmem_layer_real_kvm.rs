//! Real-KVM e2e coverage for Phase B pmem layer scenarios.
//!
//! Ignored by default. Run on a KVM host with the usual m80 preflight env:
//!
//! ```text
//! cargo test -p m80-firecracker --test pmem_layer_real_kvm -- --ignored --nocapture
//! ```
//!
//! The pmem erofs+DAX cases require a kernel with built-in erofs, virtio-pmem,
//! and FS-DAX support. The m80 stripped kernel has those symbols; the stock
//! Firecracker CI kernel reports `ENODEV` for the erofs mount.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer,
    PmemSharing, RunningSandbox, SandboxConfig, TrustDomainAck, TrustReason,
};
use m80_image_store::{ImageKind, ImageStore, DEFAULT_STORE_ROOT};
use m80_jailer::InspectionDecision;
use m80_proto::{ExecRequest, ExecStatus};

mod common;

static REAL_KVM_LOCK: Mutex<()> = Mutex::new(());

mod pmem_layer_real_kvm {
    use super::*;

    #[test]
    #[ignore = "requires KVM host, real Firecracker binary, and root privileges"]
    fn pmem_layer_per_vm_mount_dax_real_kvm() {
        let _serial = REAL_KVM_LOCK.lock().expect("real-kvm test lock");
        let store = open_default_store();
        let layer = build_layer(&store, 0);
        let discovery = m80_preflight::run()
            .expect("preflight must pass on a KVM-capable host with m80 artifacts");
        let backend = make_real_backend(discovery, 1);
        let mut running = launch_with_layers(&backend, "pmem-dax", vec![layer]);
        let run_dir = running.run_dir().to_path_buf();

        let mounts = exec_stdout(&mut running, "cat /proc/mounts");
        let mount_lines = assert_pmem_mounts(&mounts, 1);
        assert_eq!(mount_lines.len(), 1);

        let stopped = running.stop().expect("stop pmem dax VM");
        stopped.delete().expect("delete pmem dax VM");
        assert!(!run_dir.exists(), "run dir leaked: {}", run_dir.display());
        println!(
            "pmem_layer_per_vm_mount_dax_real_kvm passed: mount_line={}",
            mount_lines[0]
        );
    }

    #[test]
    #[ignore = "requires KVM host, real Firecracker binary, and root privileges"]
    fn pmem_layer_per_vm_distinct_backing_inodes_real_kvm() {
        let _serial = REAL_KVM_LOCK.lock().expect("real-kvm test lock");
        let store = open_default_store();
        let layer = build_layer(&store, 0);
        let discovery = m80_preflight::run()
            .expect("preflight must pass on a KVM-capable host with m80 artifacts");
        let backend = make_real_backend(discovery, 2);
        let first = launch_with_layers(&backend, "pmem-inode-a", vec![layer.clone()]);
        let second = launch_with_layers(&backend, "pmem-inode-b", vec![layer]);
        let first_run_dir = first.run_dir().to_path_buf();
        let second_run_dir = second.run_dir().to_path_buf();
        let first_backing = first_run_dir.join("pmem/0.img");
        let second_backing = second_run_dir.join("pmem/0.img");

        let first_inode = std::fs::metadata(&first_backing)
            .expect("first pmem backing metadata")
            .ino();
        let second_inode = std::fs::metadata(&second_backing)
            .expect("second pmem backing metadata")
            .ino();
        assert_ne!(
            first_inode, second_inode,
            "PerVm backings must be distinct inodes"
        );

        first
            .stop()
            .expect("stop first inode VM")
            .delete()
            .expect("delete first inode VM");
        second
            .stop()
            .expect("stop second inode VM")
            .delete()
            .expect("delete second inode VM");
        assert!(!first_run_dir.exists(), "first run dir leaked");
        assert!(!second_run_dir.exists(), "second run dir leaked");
        println!(
        "pmem_layer_per_vm_distinct_backing_inodes_real_kvm passed: first_inode={first_inode} second_inode={second_inode}"
    );
    }

    #[test]
    #[ignore = "requires KVM host, real Firecracker binary, and root privileges"]
    fn pmem_layer_shared_reuses_backing_inode_real_kvm() {
        let _serial = REAL_KVM_LOCK.lock().expect("real-kvm test lock");
        let store = open_default_store();
        let layer = build_layer_with_sharing(
            &store,
            0,
            PmemSharing::Shared(TrustDomainAck::new(TrustReason::SameOperator)),
        );
        let shared_store_path = store_erofs_path(&store, &layer);
        let shared_store_digest = store_digest(&layer);
        let shared_store_identity = file_identity(&shared_store_path);
        println!(
            "shared pmem smoke: built layer path={} dev={} inode={}",
            shared_store_path.display(),
            shared_store_identity.0,
            shared_store_identity.1
        );
        let discovery = m80_preflight::run()
            .expect("preflight must pass on a KVM-capable host with m80 artifacts");
        println!("shared pmem smoke: preflight passed");
        let firecracker_bin = discovery.firecracker_bin.clone();
        let backend = make_real_backend(discovery, 2);
        println!("shared pmem smoke: launching first VM");
        let mut first = launch_with_layers(&backend, "pmem-shared-a", vec![layer.clone()]);
        println!("shared pmem smoke: launching second VM");
        let mut second = launch_with_layers(&backend, "pmem-shared-b", vec![layer]);
        let first_run_dir = first.run_dir().to_path_buf();
        let second_run_dir = second.run_dir().to_path_buf();
        let first_jail_backing =
            m80_jailer::jail_root_path(&first_run_dir, &firecracker_bin).join("pmem.0.img");
        let second_jail_backing =
            m80_jailer::jail_root_path(&second_run_dir, &firecracker_bin).join("pmem.0.img");

        let first_mount_line =
            assert_pmem_mounts(&exec_stdout(&mut first, "cat /proc/mounts"), 1).remove(0);
        let second_mount_line =
            assert_pmem_mounts(&exec_stdout(&mut second, "cat /proc/mounts"), 1).remove(0);
        assert!(
            !first_run_dir.join("pmem/0.img").exists(),
            "Shared must not create first per-VM backing clone"
        );
        assert!(
            !second_run_dir.join("pmem/0.img").exists(),
            "Shared must not create second per-VM backing clone"
        );
        assert_eq!(file_identity(&first_jail_backing), shared_store_identity);
        assert_eq!(file_identity(&second_jail_backing), shared_store_identity);
        assert_eq!(
            store.shared_ref_count(&shared_store_digest).unwrap(),
            2,
            "two live shared VMs must hold two active image-store refs"
        );

        first
            .stop()
            .expect("stop first shared VM")
            .delete()
            .expect("delete first shared VM");
        assert!(shared_store_path.is_file(), "shared artifact deleted early");
        assert_eq!(
            store.shared_ref_count(&shared_store_digest).unwrap(),
            1,
            "first teardown must release only its own shared ref"
        );
        second
            .stop()
            .expect("stop second shared VM")
            .delete()
            .expect("delete second shared VM");
        assert!(
            shared_store_path.is_file(),
            "canonical shared artifact deleted"
        );
        assert_eq!(
            store.shared_ref_count(&shared_store_digest).unwrap(),
            0,
            "last teardown must release the final shared ref"
        );
        assert!(!first_run_dir.exists(), "first run dir leaked");
        assert!(!second_run_dir.exists(), "second run dir leaked");
        println!(
            "pmem_layer_shared_reuses_backing_inode_real_kvm passed: backing_path={} backing_dev={} backing_inode={} first_mount_line={} second_mount_line={}",
            shared_store_path.display(),
            shared_store_identity.0,
            shared_store_identity.1,
            first_mount_line,
            second_mount_line
        );
    }

    #[test]
    #[ignore = "requires KVM host, real Firecracker binary, and root privileges"]
    fn pmem_layer_per_vm_no_cross_vm_page_sharing_real_kvm() {
        let _serial = REAL_KVM_LOCK.lock().expect("real-kvm test lock");
        let store = open_default_store();
        let layer = build_layer(&store, 0);
        let discovery = m80_preflight::run()
            .expect("preflight must pass on a KVM-capable host with m80 artifacts");
        let backend = make_real_backend(discovery, 2);
        let mut first = launch_with_layers(&backend, "pmem-pages-a", vec![layer.clone()]);
        let mut second = launch_with_layers(&backend, "pmem-pages-b", vec![layer]);
        let first_run_dir = first.run_dir().to_path_buf();
        let second_run_dir = second.run_dir().to_path_buf();

        exec_stdout(
            &mut first,
            "cat /opt/m80-layers/smoke-0/payload.txt >/dev/null",
        );
        exec_stdout(
            &mut second,
            "cat /opt/m80-layers/smoke-0/payload.txt >/dev/null",
        );
        let first_pid = live_firecracker_pid(&first_run_dir);
        let second_pid = live_firecracker_pid(&second_run_dir);
        let first_rss = pmem_mapping_rss_kib(first_pid);
        let second_rss = pmem_mapping_rss_kib(second_pid);
        assert!(
            first_rss > 0,
            "first VM should fault pmem file-backed pages independently"
        );
        assert!(
            second_rss > 0,
            "second VM should fault pmem file-backed pages independently"
        );
        assert_ne!(
            std::fs::metadata(first_run_dir.join("pmem/0.img"))
                .expect("first backing metadata")
                .ino(),
            std::fs::metadata(second_run_dir.join("pmem/0.img"))
                .expect("second backing metadata")
                .ino(),
            "PerVm page-cache baseline depends on distinct backing inodes"
        );

        first
            .stop()
            .expect("stop first page VM")
            .delete()
            .expect("delete first page VM");
        second
            .stop()
            .expect("stop second page VM")
            .delete()
            .expect("delete second page VM");
        println!(
        "pmem_layer_per_vm_no_cross_vm_page_sharing_real_kvm passed: first_pid={first_pid} first_pmem_rss_kib={first_rss} second_pid={second_pid} second_pmem_rss_kib={second_rss}"
    );
    }

    #[test]
    #[ignore = "requires KVM host, real Firecracker binary, and root privileges"]
    fn pmem_layer_per_vm_teardown_leaves_no_leak_real_kvm() {
        let _serial = REAL_KVM_LOCK.lock().expect("real-kvm test lock");
        let store = open_default_store();
        let layer = build_layer(&store, 0);
        let discovery = m80_preflight::run()
            .expect("preflight must pass on a KVM-capable host with m80 artifacts");
        let run_root = discovery.run_root.clone();
        let backend = make_real_backend(discovery, 1);
        let mut running = launch_with_layers(&backend, "pmem-leak", vec![layer]);
        let vm_id = running.vm_id().to_owned();
        let run_dir = running.run_dir().to_path_buf();

        let bytes = exec_stdout(&mut running, "wc -c < /opt/m80-layers/smoke-0/payload.txt");
        let byte_count = bytes
            .trim()
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("wc output is not a byte count: {bytes:?}"));
        assert!(byte_count > 0, "payload should be readable from pmem layer");

        let stopped = running.stop().expect("stop pmem no-leak VM");
        stopped.delete().expect("delete pmem no-leak VM");

        assert!(!run_dir.exists(), "run dir leaked: {}", run_dir.display());
        let run_root_leaks = find_paths_named(&run_root, "pmem.0.img")
            .into_iter()
            .filter(|path| path.to_string_lossy().contains(&vm_id))
            .collect::<Vec<_>>();
        assert!(
            run_root_leaks.is_empty(),
            "pmem jail images leaked under run root: {run_root_leaks:?}"
        );
        let tmp_leaks = find_paths_with_prefix(Path::new("/tmp"), "pmem-leak");
        assert!(
            tmp_leaks.is_empty(),
            "pmem temp files leaked: {tmp_leaks:?}"
        );
        println!(
            "pmem_layer_per_vm_teardown_leaves_no_leak_real_kvm passed: vm_id={vm_id} run_dir={}",
            run_dir.display()
        );
    }
}

fn open_default_store() -> ImageStore {
    std::fs::create_dir_all(DEFAULT_STORE_ROOT).expect("create default image store root");
    ImageStore::open_default().expect("open default image store")
}

fn build_layer(store: &ImageStore, slot: usize) -> PmemLayer {
    build_layer_with_sharing(store, slot, PmemSharing::PerVm)
}

fn build_layer_with_sharing(store: &ImageStore, slot: usize, sharing: PmemSharing) -> PmemLayer {
    let source_root = tempfile::tempdir().expect("pmem source root");
    let source_dir = source_root.path().join(format!("layer-{slot}"));
    std::fs::create_dir(&source_dir).expect("pmem source dir");
    std::fs::write(
        source_dir.join("payload.txt"),
        format!("pmem payload slot {slot}\n"),
    )
    .expect("write pmem payload");
    std::fs::write(
        source_dir.join("padding.bin"),
        vec![b'x'; (4 * 1024 * 1024) - 4096],
    )
    .expect("write pmem padding payload");
    let digest = store
        .build_minimal_test_image(&source_dir, ImageKind::Erofs)
        .expect("build/import erofs pmem layer");
    let digest = ImageDigest::parse(digest.as_str()).expect("firecracker digest");
    PmemLayer::new(
        ErofsImageRef::from_digest(digest),
        sharing,
        GuestMountPath::parse(&format!("/opt/m80-layers/smoke-{slot}")).expect("guest mount path"),
    )
}

fn store_erofs_path(store: &ImageStore, layer: &PmemLayer) -> PathBuf {
    let digest = store_digest(layer);
    store
        .resolve_as(&digest, ImageKind::Erofs)
        .expect("resolve shared erofs artifact")
        .path()
        .to_path_buf()
}

fn store_digest(layer: &PmemLayer) -> m80_image_store::ImageDigest {
    m80_image_store::ImageDigest::parse(layer.image().digest().as_str()).expect("store digest")
}

fn file_identity(path: &Path) -> (u64, u64) {
    let metadata =
        std::fs::metadata(path).unwrap_or_else(|err| panic!("metadata {}: {err}", path.display()));
    (metadata.dev(), metadata.ino())
}

fn make_real_backend(discovery: m80_preflight::Discovery, max_concurrent_vms: u32) -> Arc<Backend> {
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    Arc::new(Backend::new(config).expect("Backend::new"))
}

fn launch_with_layers(
    backend: &Arc<Backend>,
    prefix: &str,
    pmem_layers: Vec<PmemLayer>,
) -> RunningSandbox {
    let vm_id = common::unique_vm_id(prefix);
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            pmem_layers,
            ..common::sandbox_config()
        })
        .expect("admit pmem sandbox");
    sandbox.launch().expect("launch pmem sandbox")
}

fn exec_stdout(running: &mut RunningSandbox, command: &str) -> String {
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_owned(),
            args: vec!["-c".to_owned(), command.to_owned()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec in pmem VM");
    assert!(
        response.status == ExecStatus::Completed && response.exit_code == Some(0),
        "exec failed for {command:?}: status={:?} exit={:?} stdout={} stderr={}",
        response.status,
        response.exit_code,
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    String::from_utf8(response.stdout).expect("exec stdout is utf8")
}

fn assert_pmem_mounts(mounts: &str, expected_layers: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in mounts.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields
            .get(1)
            .is_some_and(|mount| mount.starts_with("/opt/m80-layers/smoke-"))
        {
            lines.push((line.to_owned(), fields));
        }
    }
    assert_eq!(
        lines.len(),
        expected_layers,
        "expected {expected_layers} pmem mounts, got {lines:?}\nall mounts:\n{mounts}"
    );
    for slot in 0..expected_layers {
        let mount_path = format!("/opt/m80-layers/smoke-{slot}");
        let device = format!("/dev/pmem{slot}");
        let (line, fields) = lines
            .iter()
            .find(|(_, fields)| fields.get(1) == Some(&mount_path.as_str()))
            .unwrap_or_else(|| panic!("missing mount {mount_path}; got {lines:?}"));
        assert_eq!(fields.first().copied(), Some(device.as_str()), "{line}");
        assert_eq!(fields.get(2).copied(), Some("erofs"), "{line}");
        let options = fields.get(3).expect("mount options");
        assert!(
            options.split(',').any(|option| option == "ro"),
            "pmem mount must be read-only: {line}"
        );
        assert!(
            options
                .split(',')
                .any(|option| option == "dax" || option.starts_with("dax=")),
            "pmem mount must advertise DAX: {line}"
        );
    }
    lines.into_iter().map(|(line, _)| line).collect()
}

fn live_firecracker_pid(run_dir: &Path) -> u32 {
    match m80_jailer::inspect_run_dir(run_dir).expect("inspect live run dir") {
        InspectionDecision::LiveJail {
            firecracker_pid, ..
        } => firecracker_pid,
        other => panic!("expected live jail at {}, got {other:?}", run_dir.display()),
    }
}

fn pmem_mapping_rss_kib(firecracker_pid: u32) -> u64 {
    let smaps_path = PathBuf::from(format!("/proc/{firecracker_pid}/smaps"));
    let raw = std::fs::read_to_string(&smaps_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", smaps_path.display()));
    let mut in_pmem_mapping = false;
    let mut total = 0;
    for line in raw.lines() {
        if is_smaps_header(line) {
            in_pmem_mapping = line.contains("pmem.0.img");
            continue;
        }
        if !in_pmem_mapping {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Rss:") {
            let kib = rest
                .split_whitespace()
                .next()
                .expect("Rss value")
                .parse::<u64>()
                .expect("Rss value is integer KiB");
            total += kib;
        }
    }
    total
}

fn is_smaps_header(line: &str) -> bool {
    let Some(first) = line.as_bytes().first() else {
        return false;
    };
    first.is_ascii_hexdigit() && line.contains('-')
}

fn find_paths_named(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_paths(root, &mut found, &|path| {
        path.file_name().is_some_and(|file_name| file_name == name)
    });
    found
}

fn find_paths_with_prefix(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|file_name| file_name.to_str())
                .is_some_and(|file_name| file_name.starts_with(prefix))
        })
        .collect()
}

fn collect_paths(root: &Path, found: &mut Vec<PathBuf>, matches: &dyn Fn(&Path) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if matches(&path) {
            found.push(path.clone());
        }
        if path.is_dir() {
            collect_paths(&path, found, matches);
        }
    }
}
