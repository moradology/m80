//! Shared helpers for Phase C pmem Shared integration tests.

#![allow(dead_code)]

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer,
    PmemSharing, RunningSandbox, SandboxConfig, TrustDomainAck, TrustReason,
};
use m80_image_store::{ImageKind, ImageStore, DEFAULT_STORE_ROOT};
use m80_proto::{ExecRequest, ExecStatus};

pub(crate) static REAL_KVM_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct RealBackend {
    pub(crate) backend: Arc<Backend>,
    pub(crate) firecracker_bin: PathBuf,
}

pub(crate) fn open_default_store() -> ImageStore {
    std::fs::create_dir_all(DEFAULT_STORE_ROOT).expect("create default image store root");
    ImageStore::open_default().expect("open default image store")
}

pub(crate) fn build_test_image_digest(
    store: &ImageStore,
    slot: usize,
) -> m80_image_store::ImageDigest {
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
    store
        .build_minimal_test_image(&source_dir, ImageKind::Erofs)
        .expect("build/import erofs pmem layer")
}

pub(crate) fn build_payload_image_digest(
    store: &ImageStore,
    payload_mib: usize,
) -> m80_image_store::ImageDigest {
    assert!(payload_mib > 0, "payload_mib must be positive");
    let source_root = tempfile::tempdir().expect("pmem density source root");
    let source_dir = source_root.path().join("density-layer");
    std::fs::create_dir(&source_dir).expect("pmem density source dir");
    std::fs::write(
        source_dir.join("payload.txt"),
        format!("pmem density payload {payload_mib} MiB\n"),
    )
    .expect("write pmem density payload marker");

    let payload_path = source_dir.join("payload.bin");
    let mut file = std::fs::File::create(&payload_path).expect("create density payload");
    let chunk = vec![0x5a; 1024 * 1024];
    for _ in 0..payload_mib {
        std::io::Write::write_all(&mut file, &chunk).expect("write density payload chunk");
    }
    std::io::Write::flush(&mut file).expect("flush density payload");
    file.sync_all().expect("sync density payload");

    let digest = store
        .build_minimal_test_image(&source_dir, ImageKind::Erofs)
        .expect("build/import density erofs pmem layer");
    let image = store_erofs_path(store, &digest);
    assert_payload_file_uncompressed_non_inlined(&image, "payload.bin");
    digest
}

#[derive(Clone, Debug)]
pub(crate) struct PayloadLayout {
    pub(crate) size_bytes: u64,
    pub(crate) on_disk_size_bytes: u64,
    pub(crate) layout: u8,
    pub(crate) compression_ratio: String,
    pub(crate) raw_dump: String,
}

pub(crate) fn assert_payload_file_uncompressed_non_inlined(
    image: &Path,
    payload_path: &str,
) -> PayloadLayout {
    let raw_dump = dump_erofs_payload(image, payload_path);
    let layout = parse_payload_layout(&raw_dump);
    assert_eq!(
        layout.layout, 0,
        "{payload_path} must use erofs layout 0 for file-level DAX; dump:\n{raw_dump}"
    );
    assert_eq!(
        layout.size_bytes, layout.on_disk_size_bytes,
        "{payload_path} must be uncompressed and non-inlined for file-level DAX; dump:\n{raw_dump}"
    );
    layout
}

fn dump_erofs_payload(image: &Path, payload_path: &str) -> String {
    let output = Command::new("dump.erofs")
        .arg(format!("--path=/{payload_path}"))
        .arg(image)
        .output()
        .unwrap_or_else(|err| panic!("spawn dump.erofs for {}: {err}", image.display()));
    assert!(
        output.status.success(),
        "dump.erofs failed for {}: {}",
        image.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("dump.erofs output is utf8")
}

fn parse_payload_layout(raw: &str) -> PayloadLayout {
    let mut size_bytes = None;
    let mut on_disk_size_bytes = None;
    let mut layout = None;
    let mut compression_ratio = None;

    for line in raw.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Size:") {
            size_bytes = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok());
            on_disk_size_bytes = line
                .split("On-disk size:")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok());
        }
        if line.contains("Layout:") {
            layout = line
                .split("Layout:")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|value| value.parse::<u8>().ok());
            compression_ratio = line
                .split("Compression ratio:")
                .nth(1)
                .map(str::trim)
                .map(str::to_owned);
        }
    }

    PayloadLayout {
        size_bytes: size_bytes
            .unwrap_or_else(|| panic!("missing Size in dump.erofs output: {raw}")),
        on_disk_size_bytes: on_disk_size_bytes
            .unwrap_or_else(|| panic!("missing On-disk size in dump.erofs output: {raw}")),
        layout: layout.unwrap_or_else(|| panic!("missing Layout in dump.erofs output: {raw}")),
        compression_ratio: compression_ratio
            .unwrap_or_else(|| panic!("missing Compression ratio in dump.erofs output: {raw}")),
        raw_dump: raw.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_payload_layout_reads_plain_erofs_dump() {
        let layout = parse_payload_layout(
            "Path : /payload.bin\n\
             Size: 33554432  On-disk size: 33554432  regular file\n\
             NID: 40   Links: 1   Layout: 0   Compression ratio: 100.00%\n",
        );

        assert_eq!(layout.size_bytes, 33_554_432);
        assert_eq!(layout.on_disk_size_bytes, 33_554_432);
        assert_eq!(layout.layout, 0);
        assert_eq!(layout.compression_ratio, "100.00%");
    }
}

pub(crate) fn layer_from_digest(
    digest: &m80_image_store::ImageDigest,
    slot: usize,
    sharing: PmemSharing,
) -> PmemLayer {
    let digest = ImageDigest::parse(digest.as_str()).expect("firecracker digest");
    PmemLayer::new(
        ErofsImageRef::from_digest(digest),
        sharing,
        GuestMountPath::parse(&format!("/opt/m80-layers/smoke-{slot}")).expect("guest mount path"),
    )
}

pub(crate) fn shared_layer(digest: &m80_image_store::ImageDigest, slot: usize) -> PmemLayer {
    layer_from_digest(digest, slot, PmemSharing::Shared(shared_ack()))
}

#[allow(dead_code)]
pub(crate) fn per_vm_layer(digest: &m80_image_store::ImageDigest, slot: usize) -> PmemLayer {
    layer_from_digest(digest, slot, PmemSharing::PerVm)
}

pub(crate) fn shared_ack() -> TrustDomainAck {
    TrustDomainAck::new(TrustReason::SameOperator)
}

pub(crate) fn store_erofs_path(
    store: &ImageStore,
    digest: &m80_image_store::ImageDigest,
) -> PathBuf {
    store
        .resolve_as(digest, ImageKind::Erofs)
        .expect("resolve shared erofs artifact")
        .path()
        .to_path_buf()
}

pub(crate) fn file_identity(path: &Path) -> (u64, u64) {
    let metadata =
        std::fs::metadata(path).unwrap_or_else(|err| panic!("metadata {}: {err}", path.display()));
    (metadata.dev(), metadata.ino())
}

pub(crate) fn real_backend(max_concurrent_vms: u32) -> RealBackend {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    real_backend_from_discovery(discovery, max_concurrent_vms)
}

pub(crate) fn real_backend_from_discovery(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
) -> RealBackend {
    let firecracker_bin = discovery.firecracker_bin.clone();
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    RealBackend {
        backend: Arc::new(Backend::new(config).expect("Backend::new")),
        firecracker_bin,
    }
}

pub(crate) fn launch_with_layers(
    backend: &Arc<Backend>,
    prefix: &str,
    pmem_layers: Vec<PmemLayer>,
) -> RunningSandbox {
    let vm_id = crate::common::unique_vm_id(prefix);
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            pmem_layers,
            ..crate::common::sandbox_config()
        })
        .expect("admit pmem sandbox");
    sandbox.launch().expect("launch pmem sandbox")
}

pub(crate) fn stop_and_delete(running: RunningSandbox) {
    running
        .stop()
        .expect("stop pmem VM")
        .delete()
        .expect("delete pmem VM");
}

pub(crate) fn exec_stdout(running: &mut RunningSandbox, command: &str) -> String {
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

pub(crate) fn assert_one_pmem_mount(running: &mut RunningSandbox, slot: usize) -> String {
    let mounts = exec_stdout(running, "cat /proc/mounts");
    let mount_path = format!("/opt/m80-layers/smoke-{slot}");
    let device = format!("/dev/pmem{slot}");
    let line = mounts
        .lines()
        .find(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            fields.get(0) == Some(&device.as_str()) && fields.get(1) == Some(&mount_path.as_str())
        })
        .unwrap_or_else(|| panic!("missing mount {mount_path}; all mounts:\n{mounts}"));
    let fields = line.split_whitespace().collect::<Vec<_>>();
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
    line.to_owned()
}

pub(crate) fn jail_backing_path(run_dir: &Path, firecracker_bin: &Path, slot: usize) -> PathBuf {
    m80_jailer::jail_root_path(run_dir, firecracker_bin).join(format!("pmem.{slot}.img"))
}
