//! Post-init digest construction for snapshot-template builds.

use m80_firecracker_client::CpuTemplate;
use m80_image_manifest::{ImageKind, RootfsFormat};
use m80_snapshot_template::{PmemTemplateEntry, PmemTemplateSharing, TemplateDigest};
use sha2::{Digest as _, Sha256};

use crate::error::FcError;
use crate::types::{Backend, SandboxConfig, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PostInitDigest([u8; 32]);

impl PostInitDigest {
    pub(super) fn of(observables: &PostInitObservables) -> Self {
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, "version", b"PostInitDigestV1");
        hash_u32(&mut hasher, "proto_version", observables.proto_version);
        hash_field(&mut hasher, "image_kind", observables.image_kind.as_bytes());
        hash_field(
            &mut hasher,
            "rootfs_format",
            observables.rootfs_format.as_bytes(),
        );
        hash_field(
            &mut hasher,
            "guestd_sha256",
            observables.guestd_sha256.as_bytes(),
        );
        hash_field(
            &mut hasher,
            "rootfs_sha256",
            observables.rootfs_sha256.as_bytes(),
        );
        hash_u32(&mut hasher, "vcpu_count", observables.vcpu_count);
        hash_u32(&mut hasher, "mem_size_mib", observables.mem_size_mib);
        hash_field(
            &mut hasher,
            "cpu_template",
            observables.cpu_template.as_bytes(),
        );
        hash_field(&mut hasher, "boot_args", observables.boot_args.as_bytes());
        hash_usize(&mut hasher, "pmem_len", observables.pmem_layers.len());
        for layer in &observables.pmem_layers {
            hash_field(&mut hasher, "pmem_mount_at", layer.mount_at.as_bytes());
            hash_field(&mut hasher, "pmem_digest", layer.image_digest.as_bytes());
            hash_field(&mut hasher, "pmem_sharing", layer.sharing.as_bytes());
            hash_field(&mut hasher, "pmem_jail_path", layer.jail_path.as_bytes());
        }
        Self(hasher.finalize().into())
    }

    pub(super) fn to_template_digest(&self) -> Result<TemplateDigest, FcError> {
        Ok(TemplateDigest::parse(&hex::encode(self.0))?)
    }
}

#[derive(Debug)]
pub(super) struct PostInitObservables {
    proto_version: u32,
    image_kind: &'static str,
    rootfs_format: &'static str,
    guestd_sha256: String,
    rootfs_sha256: String,
    vcpu_count: u32,
    mem_size_mib: u32,
    cpu_template: &'static str,
    boot_args: String,
    pmem_layers: Vec<PostInitPmemObservable>,
}

impl PostInitObservables {
    pub(super) fn from_backend(
        backend: &Backend,
        sandbox_config: &SandboxConfig,
        pmem_layers: &[PmemTemplateEntry],
    ) -> Self {
        let manifest = &backend.config().discovery().manifest;
        let mut pmem_layers = pmem_layers
            .iter()
            .map(PostInitPmemObservable::from)
            .collect::<Vec<_>>();
        pmem_layers.sort();
        Self {
            proto_version: m80_proto::PROTOCOL_VERSION,
            image_kind: image_kind_name(manifest.image_kind),
            rootfs_format: rootfs_format_name(manifest.rootfs_format),
            guestd_sha256: manifest.daemon_binary_sha256.clone(),
            rootfs_sha256: manifest.output_rootfs_sha256.clone(),
            vcpu_count: sandbox_config.vcpu_count.unwrap_or(FIRST_LINE_VCPU_COUNT),
            mem_size_mib: sandbox_config
                .mem_size_mib
                .unwrap_or(FIRST_LINE_MEM_SIZE_MIB),
            cpu_template: cpu_template_name(sandbox_config.cpu_template),
            boot_args: sandbox_config.boot_args.clone().unwrap_or_default(),
            pmem_layers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PostInitPmemObservable {
    mount_at: String,
    image_digest: String,
    sharing: &'static str,
    jail_path: String,
}

impl From<&PmemTemplateEntry> for PostInitPmemObservable {
    fn from(layer: &PmemTemplateEntry) -> Self {
        Self {
            mount_at: layer.mount_at().as_path().to_string_lossy().into_owned(),
            image_digest: layer.image_digest().as_str().to_owned(),
            sharing: template_pmem_sharing_name(layer.sharing()),
            jail_path: layer
                .jail_backing_path()
                .as_path()
                .to_string_lossy()
                .into_owned(),
        }
    }
}

fn image_kind_name(kind: ImageKind) -> &'static str {
    match kind {
        ImageKind::Ubuntu => "ubuntu",
        ImageKind::Minimal => "minimal",
    }
}

fn rootfs_format_name(format: RootfsFormat) -> &'static str {
    match format {
        RootfsFormat::Ext4 => "ext4",
        RootfsFormat::Erofs => "erofs",
    }
}

fn cpu_template_name(template: Option<CpuTemplate>) -> &'static str {
    match template {
        None => "none",
        Some(CpuTemplate::T2) => "t2",
        Some(CpuTemplate::C3) => "c3",
    }
}

fn template_pmem_sharing_name(sharing: PmemTemplateSharing) -> &'static str {
    match sharing {
        PmemTemplateSharing::PerVm => "per_vm",
        PmemTemplateSharing::Shared => "shared",
    }
}

fn hash_field(hasher: &mut Sha256, label: &str, value: &[u8]) {
    hasher.update(label.as_bytes());
    hasher.update([0]);
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn hash_u32(hasher: &mut Sha256, label: &str, value: u32) {
    hash_field(hasher, label, &value.to_be_bytes());
}

fn hash_usize(hasher: &mut Sha256, label: &str, value: usize) {
    hash_field(hasher, label, &(value as u64).to_be_bytes());
}
