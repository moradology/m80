//! VM-lifecycle event log, per-VM probe, health rollup, Prometheus
//! rendering. See `README.md` for the crate contract.
//! Behavior captures: bead epic `m80-1f8`.

#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Diagnostics JSONL schema version.
pub const DIAGNOSTICS_SCHEMA_VERSION: u16 = 2;

/// File name used inside each per-VM run directory.
pub const DIAGNOSTICS_FILE_NAME: &str = "diagnostics.jsonl";

/// A handle to per-VM diagnostics. Created by [`Diagnostics::disabled`] in
/// no-op mode, or [`Diagnostics::open`] to append structured VM-lifecycle
/// events to `<run_dir>/diagnostics.jsonl`.
#[derive(Debug)]
pub struct Diagnostics {
    file: Option<File>,
    path: Option<PathBuf>,
}

impl Diagnostics {
    /// Return a no-op [`Diagnostics`] handle.
    pub fn disabled() -> Self {
        Self {
            file: None,
            path: None,
        }
    }

    /// Open `<run_dir>/diagnostics.jsonl` for append, creating it if needed.
    pub fn open(run_dir: &Path) -> Result<Self, ObservabilityError> {
        let path = run_dir.join(DIAGNOSTICS_FILE_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            file: Some(file),
            path: Some(path),
        })
    }

    /// Return the diagnostics file path when this handle is enabled.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Append one [`VmEvent`] to the diagnostics log.
    pub fn record(&mut self, event: &VmEvent) -> Result<(), ObservabilityError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        serde_json::to_writer(&mut *file, event)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::disabled()
    }
}

impl Drop for Diagnostics {
    fn drop(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
            let _ = file.sync_all();
        }
    }
}

/// One structured event for the diagnostics log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmEvent {
    /// Diagnostics schema version.
    pub schema_version: u16,
    /// Unix epoch milliseconds at the time of recording.
    pub timestamp_unix_ms: u64,
    /// Source class that emitted the event.
    pub source_class: SourceClass,
    /// Lifecycle phase the event belongs to.
    pub phase: Phase,
    /// Caller-supplied free-text message.
    pub message: String,
    /// Opaque request id when a caller request is in scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Bounded key/value context for grep-friendly triage.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, String>,
}

impl VmEvent {
    /// Build a host-side VM event with the current timestamp.
    pub fn host(
        phase: Phase,
        message: impl Into<String>,
        request_id: Option<impl Into<String>>,
        context: BTreeMap<String, String>,
    ) -> Self {
        Self {
            schema_version: DIAGNOSTICS_SCHEMA_VERSION,
            timestamp_unix_ms: unix_ms_now(),
            source_class: SourceClass::Host,
            phase,
            message: message.into(),
            request_id: request_id.map(Into::into),
            context,
        }
    }
}

/// Event source class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    /// Host-side m80 process.
    Host,
    /// Guest-side m80-guestd process.
    Guest,
}

/// VM lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Phase {
    /// `m80-firecracker-client` PUTs + `InstanceStart`.
    Boot,
    /// Run-dir teardown.
    Delete,
    /// `m80-preflight` stage.
    HostPreflight,
    /// Host-side network setup.
    NetworkPrepare,
    /// Vsock ready-marker observed.
    Ready,
    /// One caller request such as exec, PTY, stop, or warm lease.
    Request,
    /// Graceful or forced stop.
    Stop,
    /// `m80-storage` rootfs clone + scratch hydration.
    StoragePrepare,
    /// Run-root recovery loop reaped a stale run-dir.
    StartupScavenge,
    /// Caller-requested writeback/extract-changes phase.
    Writeback,
}

/// One per-VM probe record. v0.2 surface; v0.1 stub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmProbeRecord {
    /// Health classification.
    pub health: VmHealth,
    /// Path of the run-dir.
    pub run_dir: PathBuf,
    /// VM identifier.
    pub vm_id: String,
}

/// Health classification produced by the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VmHealth {
    /// Live but with degraded indicators.
    Degraded,
    /// Process tree gone; residue remains.
    Exited,
    /// Sockets responsive, ownership markers consistent.
    Healthy,
    /// Live but unresponsive.
    Stuck,
}

/// Probe over a run-root and emit one record per VM.
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn probe(_run_root: &Path) -> Result<Vec<VmProbeRecord>, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Aggregated rollup of probe records. v0.2 surface; v0.1 stub.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthSnapshot {
    /// Number of degraded VMs.
    pub degraded: u32,
    /// Number of exited VMs awaiting cleanup.
    pub exited: u32,
    /// Number of healthy VMs.
    pub healthy: u32,
    /// Number of stuck VMs.
    pub stuck: u32,
}

/// Per-VM operational metrics rolled up across the run-root. v0.2 surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpsMetrics {
    /// Number of VMs the metrics span.
    pub vm_count: u32,
}

/// Aggregate probe records into a [`HealthSnapshot`].
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn aggregate_health(_records: &[VmProbeRecord]) -> Result<HealthSnapshot, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Render a Prometheus exposition-format text response.
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn render_prometheus(
    _health: &HealthSnapshot,
    _metrics: &OpsMetrics,
) -> Result<String, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Render a JSON health snapshot.
///
/// **Returns [`ObservabilityError::Deferred`] in v0.1.**
pub fn render_health_json(_health: &HealthSnapshot) -> Result<String, ObservabilityError> {
    Err(ObservabilityError::Deferred)
}

/// Errors surfaced by observability operations.
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    /// Observability execution lane is deferred to v0.2.
    #[error("observability execution is deferred to v0.2")]
    Deferred,
    /// Underlying I/O failure.
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    /// JSON encode/decode failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_diagnostics_record_is_noop() {
        let mut diagnostics = Diagnostics::disabled();
        let event = VmEvent::host(Phase::Boot, "ignored", None::<String>, BTreeMap::new());

        diagnostics.record(&event).unwrap();
        assert_eq!(diagnostics.path(), None);
    }

    #[test]
    fn open_writes_schema_versioned_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let mut diagnostics = Diagnostics::open(dir.path()).unwrap();
        let mut context = BTreeMap::new();
        context.insert("vm_id".to_owned(), "vm-test".to_owned());

        diagnostics
            .record(&VmEvent::host(
                Phase::Request,
                "exec started",
                Some("req-01"),
                context,
            ))
            .unwrap();
        drop(diagnostics);

        let text = std::fs::read_to_string(dir.path().join(DIAGNOSTICS_FILE_NAME)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(parsed["schema_version"], DIAGNOSTICS_SCHEMA_VERSION);
        assert_eq!(parsed["source_class"], "host");
        assert_eq!(parsed["phase"], "Request");
        assert_eq!(parsed["message"], "exec started");
        assert_eq!(parsed["request_id"], "req-01");
        assert_eq!(parsed["context"]["vm_id"], "vm-test");
    }

    #[test]
    fn phase_enum_covers_documented_lifecycle_vocabulary() {
        let phases = [
            Phase::StartupScavenge,
            Phase::HostPreflight,
            Phase::StoragePrepare,
            Phase::NetworkPrepare,
            Phase::Boot,
            Phase::Ready,
            Phase::Request,
            Phase::Stop,
            Phase::Writeback,
            Phase::Delete,
        ];

        let rendered: Vec<String> = phases
            .into_iter()
            .map(|phase| {
                serde_json::to_value(phase)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            rendered,
            [
                "StartupScavenge",
                "HostPreflight",
                "StoragePrepare",
                "NetworkPrepare",
                "Boot",
                "Ready",
                "Request",
                "Stop",
                "Writeback",
                "Delete",
            ]
        );
    }
}
