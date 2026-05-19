//! Snapshot-template build path.
//!
//! The producer path lands before the warm-pool fill integration bead wires it
//! into `WarmStrategy::SnapshotRestore`; tests exercise it directly in the
//! interim.

#![allow(dead_code)]

mod post_init;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{ConfigError, FcError};
use crate::layout::pmem_layer_jail_path;
use crate::pmem::{validate_pmem_layers, PmemSharing};
use crate::types::{Backend, SandboxConfig};
use m80_snapshot::SnapshotPaths;
use m80_snapshot_template::{
    GuestMountPath as TemplateGuestMountPath, HookSpecSet, ImageDigest as TemplateImageDigest,
    JailBackingPath, PinnedTemplate, PmemTemplateEntry, PmemTemplateSharing, TemplateDigest,
    TemplateInputs, TemplateRestoreLayout, TemplateStore,
};

use post_init::{PostInitDigest, PostInitObservables};

pub(crate) fn build_template(
    backend: &Arc<Backend>,
    inputs: TemplateInputs,
    store: &TemplateStore,
    sandbox_config: &SandboxConfig,
) -> Result<PinnedTemplate, FcError> {
    lookup_or_build_template_with(
        backend,
        inputs,
        store,
        sandbox_config,
        build_template_cache_miss,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn lookup_or_build_template_with<F>(
    backend: &Arc<Backend>,
    inputs: TemplateInputs,
    store: &TemplateStore,
    sandbox_config: &SandboxConfig,
    build_miss: F,
) -> Result<PinnedTemplate, FcError>
where
    F: FnOnce(
        &Arc<Backend>,
        TemplateInputs,
        &TemplateStore,
        &SandboxConfig,
        TemplateRestoreLayout,
    ) -> Result<PinnedTemplate, FcError>,
{
    validate_template_sandbox_config(sandbox_config)?;
    ensure_inputs_match_host(backend, sandbox_config, &inputs)?;

    if let Some(pinned) = store.lookup(&inputs)? {
        return Ok(pinned);
    }

    let restore_layout = restore_layout_for_inputs(&inputs)?;
    build_miss(backend, inputs, store, sandbox_config, restore_layout)
}

fn build_template_cache_miss(
    backend: &Arc<Backend>,
    inputs: TemplateInputs,
    store: &TemplateStore,
    sandbox_config: &SandboxConfig,
    restore_layout: TemplateRestoreLayout,
) -> Result<PinnedTemplate, FcError> {
    let _image_gc_guard = template_build_image_gc_guard(&inputs)?;
    let plan = store.reserve(inputs, restore_layout)?;
    let sandbox = match backend.admit(sandbox_config.clone()) {
        Ok(sandbox) => sandbox,
        Err(err) => {
            cleanup_staging_dir(plan.staging_dir());
            return Err(err);
        }
    };
    let mut running = match sandbox.launch() {
        Ok(running) => running,
        Err(err) => {
            cleanup_staging_dir(plan.staging_dir());
            return Err(err);
        }
    };

    let capture =
        TemplateCaptureTarget::new(backend.config().run_root(), &plan.fingerprint().to_hex());

    if let Err(err) = running.capture(capture.paths()) {
        discard_running(running);
        capture.cleanup();
        cleanup_staging_dir(plan.staging_dir());
        return Err(err);
    }
    if let Err(err) = capture.publish_to(plan.body_paths()) {
        discard_running(running);
        capture.cleanup();
        cleanup_staging_dir(plan.staging_dir());
        return Err(err);
    }
    if fail_after_capture_requested() {
        discard_running(running);
        cleanup_staging_dir(plan.staging_dir());
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "template.build",
            reason: "injected failure after capture".to_owned(),
        }));
    }

    let staging_dir = plan.staging_dir().to_path_buf();
    let stopped = match running.stop() {
        Ok(stopped) => stopped,
        Err(err) => {
            cleanup_staging_dir(&staging_dir);
            return Err(err);
        }
    };
    if let Err(err) = stopped.delete() {
        cleanup_staging_dir(&staging_dir);
        return Err(err);
    }
    match store.commit(plan) {
        Ok(pinned) => Ok(pinned),
        Err(err) => {
            cleanup_staging_dir(&staging_dir);
            Err(err.into())
        }
    }
}

fn template_build_image_gc_guard(
    inputs: &TemplateInputs,
) -> Result<Option<m80_image_store::ImageTemplateCoordinationGuard>, FcError> {
    if inputs.pmem_image_digest_set().is_empty() {
        return Ok(None);
    }
    let store = m80_image_store::ImageStore::open_default()?;
    Ok(Some(store.acquire_template_build_guard()?))
}

struct TemplateCaptureTarget {
    dir: PathBuf,
}

impl TemplateCaptureTarget {
    fn new(run_root: &Path, fingerprint: &str) -> Self {
        Self {
            dir: run_root
                .join(".template-capture")
                .join(format!("{fingerprint}-{}", std::process::id())),
        }
    }

    fn paths(&self) -> SnapshotPaths {
        SnapshotPaths {
            vm_state: self.dir.join("vm.snap"),
            mem: self.dir.join("mem.snap"),
        }
    }

    fn publish_to(
        &self,
        body_paths: &m80_snapshot_template::TemplateBodyPaths,
    ) -> Result<(), FcError> {
        std::fs::rename(self.dir.join("vm.snap"), &body_paths.vm_state).map_err(|source| {
            FcError::PathIo {
                path: body_paths.vm_state.clone(),
                source,
            }
        })?;
        std::fs::rename(self.dir.join("mem.snap"), &body_paths.mem).map_err(|source| {
            FcError::PathIo {
                path: body_paths.mem.clone(),
                source,
            }
        })?;
        self.cleanup();
        Ok(())
    }

    fn cleanup(&self) {
        let parent = self.dir.parent().map(Path::to_path_buf);
        if let Err(source) = std::fs::remove_dir_all(&self.dir) {
            if source.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %self.dir.display(), error = %source, "failed to remove template capture dir");
            }
        }
        if let Some(parent) = parent {
            cleanup_empty_capture_parent(&parent);
        }
    }
}

fn cleanup_empty_capture_parent(parent: &Path) {
    if let Err(source) = std::fs::remove_dir(parent) {
        if source.kind() != std::io::ErrorKind::NotFound && !is_directory_not_empty(&source) {
            tracing::warn!(path = %parent.display(), error = %source, "failed to remove template capture parent");
        }
    }
}

fn is_directory_not_empty(source: &std::io::Error) -> bool {
    const ENOTEMPTY: i32 = 39;
    const EEXIST: i32 = 17;
    matches!(source.raw_os_error(), Some(ENOTEMPTY | EEXIST))
}

pub(crate) fn template_inputs_for_current_host(
    backend: &Backend,
    sandbox_config: &SandboxConfig,
    hooks: HookSpecSet,
) -> Result<TemplateInputs, FcError> {
    validate_template_sandbox_config(sandbox_config)?;
    let pmem_entries = pmem_template_entries(sandbox_config)?;
    let post_init_state_digest = PostInitDigest::of(&PostInitObservables::from_backend(
        backend,
        sandbox_config,
        &pmem_entries,
    ))
    .to_template_digest()?;
    let manifest = &backend.config().discovery().manifest;
    Ok(TemplateInputs::new(
        host_kernel_release()?,
        manifest.expected_firecracker_version.clone(),
        TemplateDigest::parse(&manifest.kernel_image_sha256)?,
        pmem_entries,
        post_init_state_digest,
        hooks,
    )?)
}

pub(super) fn restore_layout_for_inputs(
    inputs: &TemplateInputs,
) -> Result<TemplateRestoreLayout, FcError> {
    Ok(TemplateRestoreLayout::new(
        JailBackingPath::parse("/snapshot/vm.snap")?,
        JailBackingPath::parse("/snapshot/mem.snap")?,
        inputs.pmem_image_digest_set().to_vec(),
    ))
}

fn ensure_inputs_match_host(
    backend: &Backend,
    sandbox_config: &SandboxConfig,
    inputs: &TemplateInputs,
) -> Result<(), FcError> {
    let live =
        template_inputs_for_current_host(backend, sandbox_config, inputs.hook_spec_set().clone())?;
    if &live == inputs {
        return Ok(());
    }
    Err(FcError::Config(ConfigError::InvalidValue {
        field: "template.inputs",
        reason: "must match host discovery, sandbox config, and computed post-init digest"
            .to_owned(),
    }))
}

fn pmem_template_entries(config: &SandboxConfig) -> Result<Vec<PmemTemplateEntry>, FcError> {
    config
        .pmem_layers
        .iter()
        .enumerate()
        .map(|(slot, layer)| {
            Ok(PmemTemplateEntry::new(
                TemplateGuestMountPath::parse(&layer.mount_at().as_path().to_string_lossy())?,
                TemplateImageDigest::parse(layer.image().digest().as_str())?,
                match layer.sharing() {
                    PmemSharing::PerVm => PmemTemplateSharing::PerVm,
                    PmemSharing::Shared(_) => PmemTemplateSharing::Shared,
                },
                JailBackingPath::parse(pmem_layer_jail_path(slot))?,
            ))
        })
        .collect()
}

fn validate_template_sandbox_config(config: &SandboxConfig) -> Result<(), FcError> {
    if config.workspace.is_some() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "template.sandbox.workspace",
            reason: "template builds must be stateless".to_owned(),
        }));
    }
    validate_pmem_layers(&config.pmem_layers)
}

fn cleanup_staging_dir(staging_dir: &Path) {
    if let Err(source) = std::fs::remove_dir_all(staging_dir) {
        if source.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %staging_dir.display(), error = %source, "failed to remove template staging dir");
        }
    }
}

fn discard_running(running: crate::types::RunningSandbox) {
    if let Err(err) = running.force_kill().and_then(|stopped| stopped.delete()) {
        tracing::warn!(error = %err, "failed to discard sandbox after template build failure");
    }
}

fn host_kernel_release() -> Result<String, FcError> {
    let uts = nix::sys::utsname::uname().map_err(|err| {
        FcError::Config(ConfigError::InvalidValue {
            field: "template.host_kernel_version",
            reason: format!("uname failed: {err}"),
        })
    })?;
    Ok(uts.release().to_string_lossy().into_owned())
}

fn fail_after_capture_requested() -> bool {
    #[cfg(test)]
    {
        FAIL_AFTER_CAPTURE.swap(false, std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(not(test))]
    {
        false
    }
}

#[cfg(test)]
static FAIL_AFTER_CAPTURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(super) fn fail_next_template_build_after_capture_for_test() {
    FAIL_AFTER_CAPTURE.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests;
