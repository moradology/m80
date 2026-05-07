//! Warm-pool lease handle.

use std::path::Path;
use std::sync::Arc;

use m80_proto::{ExecExit, ExecRequest, ExecResponse};

use super::{discard_sandbox, WarmPoolInner};
use crate::error::FcError;
use crate::types::{ExecChunk, RunningSandbox};

/// A leased warm-pool slot.
pub struct WarmLease {
    sandbox: Option<RunningSandbox>,
    pool: Arc<WarmPoolInner>,
    released: bool,
}

impl WarmLease {
    pub(super) fn new(sandbox: RunningSandbox, pool: Arc<WarmPoolInner>) -> Self {
        Self {
            sandbox: Some(sandbox),
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

    /// VM id for the leased slot.
    pub fn vm_id(&self) -> &str {
        self.sandbox
            .as_ref()
            .expect("warm lease holds sandbox until discard")
            .vm_id()
    }

    /// Run directory for the leased slot. Useful for diagnostics before a
    /// failed lease is discarded.
    pub fn run_dir(&self) -> &Path {
        self.sandbox
            .as_ref()
            .expect("warm lease holds sandbox until discard")
            .run_dir()
    }

    /// Consume the lease, force-kill the slot, delete its run-dir, and allow
    /// the pool to refill a replacement.
    pub fn discard(mut self) -> Result<(), FcError> {
        let sandbox = self.sandbox.take();
        let result = if let Some(sandbox) = sandbox {
            discard_sandbox(sandbox)
        } else {
            Ok(())
        };
        self.release_and_refill();
        result
    }

    fn sandbox_mut(&mut self) -> Result<&mut RunningSandbox, FcError> {
        self.sandbox.as_mut().ok_or(FcError::OneShotConsumed)
    }

    fn is_one_shot(&self) -> bool {
        self.sandbox
            .as_ref()
            .map(|sandbox| sandbox.one_shot)
            .unwrap_or(false)
    }

    fn exec_and_discard<T>(
        &mut self,
        f: impl FnOnce(&mut RunningSandbox) -> Result<T, FcError>,
    ) -> Result<T, FcError> {
        let Some(mut sandbox) = self.sandbox.take() else {
            return Err(FcError::OneShotConsumed);
        };
        let result = f(&mut sandbox);
        let discard_result = discard_sandbox(sandbox);
        self.release_and_refill();
        match (result, discard_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(err), Ok(())) => Err(err),
            (Err(err), Err(cleanup)) => {
                tracing::warn!(
                    error = %cleanup,
                    "failed to discard one-shot warm lease after exec error"
                );
                Err(err)
            }
        }
    }

    fn release_and_refill(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        self.pool.lease_finished();
        self.pool.start_background_fill();
    }
}

impl Drop for WarmLease {
    fn drop(&mut self) {
        if let Some(sandbox) = self.sandbox.take() {
            let _ = discard_sandbox(sandbox);
            self.release_and_refill();
        }
    }
}

fn with_sandbox_request_id<T>(
    sandbox: &mut RunningSandbox,
    request_id: String,
    f: impl FnOnce(&mut RunningSandbox) -> Result<T, FcError>,
) -> Result<T, FcError> {
    let old = sandbox.request_id.replace(request_id);
    let result = f(sandbox);
    sandbox.request_id = old;
    result
}
