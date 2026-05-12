use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ObservabilityError;

/// Diagnostics JSONL schema version.
pub(crate) const DIAGNOSTICS_SCHEMA_VERSION: u16 = 2;

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
    #[must_use] pub fn disabled() -> Self {
        Self {
            file: None,
            path: None,
        }
    }

    /// Open `<run_dir>/diagnostics.jsonl` for append, creating it if needed.
    pub fn open(run_dir: &Path) -> Result<Self, ObservabilityError> {
        let path = run_dir.join(DIAGNOSTICS_FILE_NAME);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| ObservabilityError::PathIo {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            file: Some(file),
            path: Some(path),
        })
    }

    /// Append one [`VmEvent`] to the diagnostics log.
    pub fn record(&mut self, event: &VmEvent) -> Result<(), ObservabilityError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        serde_json::to_writer(&mut *file, event)?;
        let path = self
            .path
            .as_ref()
            .expect("file handle has diagnostics path");
        file.write_all(b"\n")
            .map_err(|source| ObservabilityError::PathIo {
                path: path.clone(),
                source,
            })?;
        file.flush().map_err(|source| ObservabilityError::PathIo {
            path: path.clone(),
            source,
        })?;
        Ok(())
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
#[serde(deny_unknown_fields)]
pub struct VmEvent {
    /// Diagnostics schema version.
    pub(crate) schema_version: u16,
    /// Unix epoch milliseconds at the time of recording.
    pub(crate) timestamp_unix_ms: u64,
    /// Event kind.
    #[serde(default)]
    pub(crate) event_kind: EventKind,
    /// Source class that emitted the event.
    pub(crate) source_class: SourceClass,
    /// Lifecycle phase the event belongs to.
    pub(crate) phase: Phase,
    /// Caller-supplied free-text message.
    pub(crate) message: String,
    /// Opaque request id when a caller request is in scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) request_id: Option<String>,
    /// Bounded key/value context for grep-friendly triage.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) context: BTreeMap<String, String>,
    /// Duration for completed phase events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) duration_us: Option<u64>,
    /// Completion outcome for completed phase or stop events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) outcome: Option<PhaseOutcome>,
    /// Typed stop/capture reason when the event is stop evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit_reason: Option<ExitReason>,
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
            event_kind: EventKind::Lifecycle,
            source_class: SourceClass::Host,
            phase,
            message: message.into(),
            request_id: request_id.map(Into::into),
            context,
            duration_us: None,
            outcome: None,
            exit_reason: None,
        }
    }

    /// Build a host-side phase-start event.
    pub fn phase_started(
        phase: Phase,
        message: impl Into<String>,
        request_id: Option<impl Into<String>>,
        context: BTreeMap<String, String>,
    ) -> Self {
        Self {
            event_kind: EventKind::PhaseStarted,
            ..Self::host(phase, message, request_id, context)
        }
    }

    /// Build a host-side phase-completed event.
    pub fn phase_completed(
        phase: Phase,
        message: impl Into<String>,
        request_id: Option<impl Into<String>>,
        context: BTreeMap<String, String>,
        duration_us: u64,
        outcome: PhaseOutcome,
    ) -> Self {
        Self {
            event_kind: EventKind::PhaseCompleted,
            duration_us: Some(duration_us),
            outcome: Some(outcome),
            ..Self::host(phase, message, request_id, context)
        }
    }

    /// Attach typed stop/capture evidence.
    #[must_use] pub fn with_exit_reason(mut self, reason: ExitReason) -> Self {
        self.exit_reason = Some(reason);
        self
    }
}

/// Diagnostics event kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum EventKind {
    /// Human-readable lifecycle milestone.
    #[default]
    Lifecycle,
    /// A phase started.
    PhaseStarted,
    /// A phase completed.
    PhaseCompleted,
}

/// Completion outcome for typed phase and stop evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "status")]
pub enum PhaseOutcome {
    /// Phase completed successfully.
    Ok,
    /// Phase timed out.
    TimedOut,
    /// Phase failed with a typed error class.
    Err {
        /// Error class, not the full free-text error message.
        class: String,
    },
}

/// Typed reason for stop-phase evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "reason")]
pub enum ExitReason {
    /// Guestd acknowledged shutdown and the host then killed Firecracker.
    NormalStop,
    /// Host killed Firecracker/jailer without a guest shutdown ack.
    ForceKill,
    /// Snapshot capture paused the VM and wrote snapshot artifacts.
    SnapshotCapture,
    /// VMM exited with a host-visible exit code.
    VmmExited {
        /// Process exit code.
        code: i32,
    },
}

/// Event source class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum SourceClass {
    /// Host-side m80 process.
    Host,
    /// Guest-side m80-guestd process.
    Guest,
}

/// VM lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
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
        assert_eq!(parsed["event_kind"], "lifecycle");
        assert_eq!(parsed["source_class"], "host");
        assert_eq!(parsed["phase"], "Request");
        assert_eq!(parsed["message"], "exec started");
        assert_eq!(parsed["request_id"], "req-01");
        assert_eq!(parsed["context"]["vm_id"], "vm-test");
    }

    #[test]
    fn phase_completed_event_carries_duration_and_outcome() {
        let event = VmEvent::phase_completed(
            Phase::Boot,
            "phase_12a_instance_start completed",
            None::<String>,
            BTreeMap::new(),
            42,
            PhaseOutcome::Ok,
        );
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["event_kind"], "phase_completed");
        assert_eq!(value["duration_us"], 42);
        assert_eq!(value["outcome"]["status"], "ok");
    }

    #[test]
    fn stop_event_carries_typed_exit_reason() {
        let event = VmEvent::host(
            Phase::Stop,
            "stop complete",
            None::<String>,
            BTreeMap::new(),
        )
        .with_exit_reason(ExitReason::NormalStop);
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["exit_reason"]["reason"], "normal_stop");
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
                "Delete",
            ]
        );
    }
}
