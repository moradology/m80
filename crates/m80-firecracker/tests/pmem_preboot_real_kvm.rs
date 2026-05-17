//! Real-KVM smoke for phase-11 pmem attach plus phase-13 guest DAX mount.
//!
//! This test is ignored by default. Run it through `scripts/smoke.sh` with:
//!
//! ```text
//! M80_PMEM_LAYERS=2 M80_PHASE_TRACE=1 ./scripts/smoke.sh launch-only
//! ```
//!

use std::path::PathBuf;

use m80_firecracker::{
    ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer, PmemSharing, SandboxConfig,
    MAX_PMEM_LAYERS,
};
use m80_image_store::{ImageKind, ImageStore, DEFAULT_STORE_ROOT};
use m80_proto::{ExecRequest, ExecStatus};

mod common;

#[test]
#[ignore = "requires KVM host, real Firecracker binary, and M80_PMEM_LAYERS or M80_PMEM_EROFS_IMAGE"]
fn pmem_layer_real_kvm_mounts_erofs_dax_before_workload() {
    std::fs::create_dir_all(DEFAULT_STORE_ROOT).expect("create default image store root");
    let store = ImageStore::open_default().expect("open default image store");
    let layers = smoke_layers(&store);

    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let backend = common::make_backend(discovery);
    let vm_id = common::unique_vm_id("pmem-preboot");
    let run_dir = backend.config().run_root().join(&vm_id);
    let jail_root =
        m80_jailer::jail_root_path(&run_dir, &backend.config().discovery().firecracker_bin);
    for (slot, layer) in layers.iter().enumerate() {
        println!(
            "pmem layer digest: slot={slot} digest={}",
            layer.image().digest().as_str()
        );
        println!(
            "pmem layer jail path: slot={slot} path={}",
            jail_root.join(format!("pmem.{slot}.img")).display()
        );
    }

    let sandbox_config = SandboxConfig {
        vm_id: Some(vm_id.clone()),
        pmem_layers: layers.clone(),
        ..common::sandbox_config()
    };

    let sandbox = backend.admit(sandbox_config).expect("admit");
    let mut running = sandbox.launch().expect("launch with pmem layer");

    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_owned(),
            args: vec!["-c".to_owned(), "cat /proc/mounts".to_owned()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("read /proc/mounts inside guest");
    assert!(
        response.status == ExecStatus::Completed && response.exit_code == Some(0),
        "guest mount lookup failed: status={:?} exit={:?} stdout={} stderr={}",
        response.status,
        response.exit_code,
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    let mounts = String::from_utf8(response.stdout).expect("mounts are utf8");
    assert_mounts(&mounts, layers.len());

    let stopped = running.stop().expect("stop pmem smoke VM");
    stopped.delete().expect("delete pmem smoke run-dir");
    assert!(
        !run_dir.join("pmem").exists(),
        "pmem backing dir must not leak after delete: {}",
        run_dir.join("pmem").display()
    );
    assert!(
        !run_dir.exists(),
        "run dir must not leak after delete: {}",
        run_dir.display()
    );

    println!(
        "pmem no-leak teardown passed: vm_id={vm_id} run_dir={}",
        run_dir.display()
    );
    println!(
        "pmem layer mount smoke passed: vm_id={vm_id} layers={}",
        layers.len()
    );
}

fn smoke_layers(store: &ImageStore) -> Vec<PmemLayer> {
    let requested = std::env::var("M80_PMEM_LAYERS")
        .ok()
        .map(|raw| {
            raw.parse::<usize>()
                .unwrap_or_else(|_| panic!("M80_PMEM_LAYERS must be an integer, got {raw:?}"))
        })
        .unwrap_or(0);

    if requested > 0 {
        assert!(
            requested <= MAX_PMEM_LAYERS,
            "M80_PMEM_LAYERS must be <= {MAX_PMEM_LAYERS}, got {requested}"
        );
        return generated_layers(store, requested);
    }

    let pmem_image =
        std::env::var("M80_PMEM_EROFS_IMAGE").expect("M80_PMEM_EROFS_IMAGE must be set");
    let pmem_image = PathBuf::from(pmem_image);
    assert!(
        pmem_image.is_file(),
        "M80_PMEM_EROFS_IMAGE must name an erofs image file: {}",
        pmem_image.display()
    );
    let store_digest = store
        .import_existing(&pmem_image, ImageKind::Erofs)
        .expect("import pmem erofs image into default store");
    vec![layer_from_store_digest(store_digest.as_str(), 0)]
}

fn generated_layers(store: &ImageStore, count: usize) -> Vec<PmemLayer> {
    let source_root = tempfile::tempdir().expect("pmem layer source root");
    (0..count)
        .map(|slot| {
            let source_dir = source_root.path().join(format!("layer-{slot}"));
            std::fs::create_dir(&source_dir).expect("layer source dir");
            std::fs::write(
                source_dir.join("payload.txt"),
                format!("pmem smoke payload slot {slot}\n"),
            )
            .expect("layer payload");
            let digest = store
                .build_minimal_test_image(&source_dir, ImageKind::Erofs)
                .expect("build and import generated erofs pmem layer");
            layer_from_store_digest(digest.as_str(), slot)
        })
        .collect()
}

fn layer_from_store_digest(digest: &str, slot: usize) -> PmemLayer {
    let layer_digest = ImageDigest::parse(digest).expect("firecracker digest");
    PmemLayer::new(
        ErofsImageRef::from_digest(layer_digest),
        PmemSharing::PerVm,
        GuestMountPath::parse(&format!("/opt/m80-layers/smoke-{slot}"))
            .expect("guest pmem mount path"),
    )
}

fn assert_mounts(mounts: &str, expected_layers: usize) {
    let mut pmem_mounts = Vec::new();
    for line in mounts.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields
            .get(1)
            .is_some_and(|mount| mount.starts_with("/opt/m80-layers/smoke-"))
        {
            pmem_mounts.push((line, fields));
        }
    }
    assert_eq!(
        pmem_mounts.len(),
        expected_layers,
        "expected {expected_layers} pmem DAX mounts, got {pmem_mounts:?}\nall mounts:\n{mounts}"
    );

    for slot in 0..expected_layers {
        let mount_path = format!("/opt/m80-layers/smoke-{slot}");
        let device = format!("/dev/pmem{slot}");
        let (line, fields) = pmem_mounts
            .iter()
            .find(|(_, fields)| fields.get(1) == Some(&mount_path.as_str()))
            .unwrap_or_else(|| panic!("missing mount path {mount_path}; got {pmem_mounts:?}"));
        assert_eq!(fields.first().copied(), Some(device.as_str()), "{line}");
        assert_eq!(fields.get(2).copied(), Some("erofs"), "{line}");
        let options = fields
            .get(3)
            .unwrap_or_else(|| panic!("mount options missing: {line}"));
        assert!(
            options.split(',').any(|option| option == "ro"),
            "pmem layer must be read-only: {line}"
        );
        assert!(
            options
                .split(',')
                .any(|option| option == "dax" || option.starts_with("dax=")),
            "pmem erofs mount must advertise DAX: {line}"
        );
        println!("pmem layer mount line: slot={slot} mount_line={line}");
    }
}
