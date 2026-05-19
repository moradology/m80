//! Warm-pool lease handle.

use std::path::Path;
use std::sync::Arc;

use m80_proto::{DirEntry, ExecExit, ExecRequest, ExecResponse, FileStat};

use super::{discard_sandbox, discard_sandbox_with_diagnostics, WarmPoolInner, WarmSlot};
use crate::error::FcError;
use crate::hotplug_types::{HotplugDriveAttach, HotplugDriveDetach};
use crate::types::{ExecChunk, RunningSandbox};

/// A leased warm-pool slot.
pub struct WarmLease {
    slot: Option<WarmSlot>,
    pool: Arc<WarmPoolInner>,
    released: bool,
}

impl WarmLease {
    pub(super) fn new(slot: WarmSlot, pool: Arc<WarmPoolInner>) -> Self {
        Self {
            slot: Some(slot),
            pool,
            released: false,
        }
    }

    /// Run one exec request on the leased slot.
    pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError> {
        if self.is_one_shot() {
            return self.exec_and_discard(|sandbox| sandbox.exec(req));
        }
        self.sandbox_mut()?.exec(req)
    }

    /// Run one exec request on the leased slot with a caller-supplied opaque
    /// request id for wire frames and diagnostics.
    pub fn exec_with_request_id(
        &mut self,
        req: ExecRequest,
        request_id: impl Into<String>,
    ) -> Result<ExecResponse, FcError> {
        if self.is_one_shot() {
            let request_id = request_id.into();
            return self.exec_and_discard(|sandbox| {
                with_sandbox_request_id(sandbox, request_id, |sandbox| sandbox.exec(req))
            });
        }
        let sandbox = self.sandbox_mut()?;
        with_sandbox_request_id(sandbox, request_id.into(), |sandbox| sandbox.exec(req))
    }

    /// Run one exec request on the leased slot and forward stdout/stderr
    /// chunks as guestd emits them.
    pub fn exec_streaming(
        &mut self,
        req: ExecRequest,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        if self.is_one_shot() {
            return self.exec_and_discard(|sandbox| sandbox.exec_streaming(req, on_chunk));
        }
        self.sandbox_mut()?.exec_streaming(req, on_chunk)
    }

    /// Run one streaming exec request on the leased slot with a caller-supplied
    /// opaque request id for wire frames and diagnostics.
    pub fn exec_streaming_with_request_id(
        &mut self,
        req: ExecRequest,
        request_id: impl Into<String>,
        on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
    ) -> Result<ExecExit, FcError> {
        if self.is_one_shot() {
            let request_id = request_id.into();
            return self.exec_and_discard(|sandbox| {
                with_sandbox_request_id(sandbox, request_id, |sandbox| {
                    sandbox.exec_streaming(req, on_chunk)
                })
            });
        }
        let sandbox = self.sandbox_mut()?;
        with_sandbox_request_id(sandbox, request_id.into(), |sandbox| {
            sandbox.exec_streaming(req, on_chunk)
        })
    }

    /// Attach and identity-verify a tenant drive before running the lease's
    /// workload. On failure the underlying sandbox has already been discarded
    /// by `RunningSandbox::attach_drive_verified`; the lease is released and
    /// the pool starts refilling a replacement.
    pub fn attach_drive_verified(&mut self, request: HotplugDriveAttach) -> Result<(), FcError> {
        let Some(slot) = self.slot.take() else {
            return Err(FcError::OneShotConsumed);
        };
        let WarmSlot {
            sandbox,
            cpuset_cpus,
        } = slot;
        match sandbox.attach_drive_verified(request) {
            Ok(sandbox) => {
                self.slot = Some(WarmSlot {
                    sandbox,
                    cpuset_cpus,
                });
                Ok(())
            }
            Err(err) => {
                self.release_and_refill(cpuset_cpus);
                Err(err)
            }
        }
    }

    /// Detach one previously attached tenant drive and retarget the slot back
    /// to its placeholder backing file. Failure discards the underlying VM and
    /// releases the lease.
    pub fn detach_drive(&mut self, request: HotplugDriveDetach) -> Result<(), FcError> {
        let Some(slot) = self.slot.take() else {
            return Err(FcError::OneShotConsumed);
        };
        let WarmSlot {
            sandbox,
            cpuset_cpus,
        } = slot;
        match sandbox.detach_drive(request) {
            Ok(sandbox) => {
                self.slot = Some(WarmSlot {
                    sandbox,
                    cpuset_cpus,
                });
                Ok(())
            }
            Err(err) => {
                self.release_and_refill(cpuset_cpus);
                Err(err)
            }
        }
    }

    /// Read a guest file directly through the leased slot.
    pub fn read_file(
        &mut self,
        path: impl Into<String>,
        max_bytes: Option<u64>,
    ) -> Result<(Vec<u8>, bool), FcError> {
        self.sandbox_mut()?.read_file(path, max_bytes)
    }

    /// Write one guest file directly through the leased slot.
    pub fn write_file(
        &mut self,
        path: impl Into<String>,
        bytes: Vec<u8>,
        mode: Option<u32>,
    ) -> Result<u64, FcError> {
        self.sandbox_mut()?.write_file(path, bytes, mode)
    }

    /// List one guest directory level through the leased slot.
    pub fn list_dir(&mut self, path: impl Into<String>) -> Result<Vec<DirEntry>, FcError> {
        self.sandbox_mut()?.list_dir(path)
    }

    /// Stat one guest path through the leased slot without following the final
    /// symlink component.
    pub fn stat_file(&mut self, path: impl Into<String>) -> Result<FileStat, FcError> {
        self.sandbox_mut()?.stat_file(path)
    }

    /// Create one guest directory through the leased slot.
    pub fn create_dir(
        &mut self,
        path: impl Into<String>,
        mode: Option<u32>,
        recursive: bool,
    ) -> Result<bool, FcError> {
        self.sandbox_mut()?.create_dir(path, mode, recursive)
    }

    /// Remove one non-directory guest path through the leased slot.
    pub fn remove_file(&mut self, path: impl Into<String>) -> Result<(), FcError> {
        self.sandbox_mut()?.remove_file(path)
    }

    /// Upload a guest file through the chunked file-write protocol.
    pub fn upload_file_chunked(
        &mut self,
        path: impl Into<String>,
        mode: Option<u32>,
        reader: impl std::io::Read,
        chunk_size: usize,
    ) -> Result<u64, FcError> {
        self.sandbox_mut()?
            .upload_file_chunked(path, mode, reader, chunk_size)
    }

    /// VM id for the leased slot.
    pub fn vm_id(&self) -> &str {
        self.slot
            .as_ref()
            .expect("warm lease holds sandbox until discard")
            .sandbox
            .vm_id()
    }

    /// Run directory for the leased slot. Useful for diagnostics before a
    /// failed lease is discarded.
    pub fn run_dir(&self) -> &Path {
        self.slot
            .as_ref()
            .expect("warm lease holds sandbox until discard")
            .sandbox
            .run_dir()
    }

    /// Consume the lease, force-kill the slot, delete its run-dir, and allow
    /// the pool to refill a replacement.
    pub fn discard(mut self) -> Result<(), FcError> {
        let slot = self.slot.take();
        let (result, cpuset_cpus) = if let Some(slot) = slot {
            let WarmSlot {
                sandbox,
                cpuset_cpus,
            } = slot;
            (discard_sandbox(sandbox), cpuset_cpus)
        } else {
            (Ok(()), None)
        };
        self.release_and_refill(cpuset_cpus);
        result
    }

    fn sandbox_mut(&mut self) -> Result<&mut RunningSandbox, FcError> {
        self.slot
            .as_mut()
            .map(|slot| &mut slot.sandbox)
            .ok_or(FcError::OneShotConsumed)
    }

    fn is_one_shot(&self) -> bool {
        self.slot.as_ref().is_some_and(|slot| slot.sandbox.one_shot)
    }

    fn exec_and_discard<T>(
        &mut self,
        f: impl FnOnce(&mut RunningSandbox) -> Result<T, FcError>,
    ) -> Result<T, FcError> {
        let Some(slot) = self.slot.take() else {
            return Err(FcError::OneShotConsumed);
        };
        let WarmSlot {
            mut sandbox,
            cpuset_cpus,
        } = slot;
        let result = f(&mut sandbox);
        let discard_result = discard_sandbox_with_diagnostics(sandbox, result.as_ref().err());
        self.release_and_refill(cpuset_cpus);
        match (result, discard_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(value), Err(cleanup)) => {
                tracing::error!(
                    error = %cleanup,
                    "failed to discard one-shot warm lease after exec success"
                );
                Ok(value)
            }
            (Err(err), Ok(())) => Err(err),
            (Err(err), Err(cleanup)) => {
                tracing::error!(
                    error = %cleanup,
                    "failed to discard one-shot warm lease after exec error"
                );
                Err(err)
            }
        }
    }

    fn release_and_refill(&mut self, cpuset_cpus: Option<String>) {
        if self.released {
            return;
        }
        self.released = true;
        self.pool.lease_finished(cpuset_cpus);
        self.pool.start_background_fill();
    }
}

impl Drop for WarmLease {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let WarmSlot {
                sandbox,
                cpuset_cpus,
            } = slot;
            if let Err(err) = discard_sandbox(sandbox) {
                tracing::error!(error = %err, "failed to discard warm lease during drop");
            }
            self.release_and_refill(cpuset_cpus);
        }
    }
}

fn with_sandbox_request_id<T>(
    sandbox: &mut RunningSandbox,
    request_id: String,
    f: impl FnOnce(&mut RunningSandbox) -> Result<T, FcError>,
) -> Result<T, FcError> {
    let old = sandbox.request_id.replace(request_id);
    // catch_unwind ensures `old` is restored even when `f` panics.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(sandbox)));
    sandbox.request_id = old;
    match result {
        Ok(val) => val,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
