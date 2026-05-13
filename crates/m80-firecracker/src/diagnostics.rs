//! Per-run diagnostics helpers.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::time::Instant;

use m80_observability::{Diagnostics, ExitReason, Phase, PhaseOutcome, VmEvent};

/// Open diagnostics for one VM run-dir.
pub(crate) fn open(run_dir: &Path, vm_id: &str, request_id: Option<&str>) -> Option<Diagnostics> {
    match Diagnostics::open(run_dir) {
        Ok(diagnostics) => {
            let mut opt = Some(diagnostics);
            record_owned(
                &mut opt,
                Phase::StartupScavenge,
                vm_id,
                request_id,
                "diagnostics opened",
            );
            opt
        }
        Err(e) => {
            tracing::warn!(
                path = %run_dir.display(),
                vm_id,
                err = %e,
                "diagnostics open failed; continuing without diagnostics"
            );
            None
        }
    }
}

/// Record one host-side lifecycle event. Any write failure disables the handle
/// for this VM so diagnostics never break boot, exec, stop, or delete.
pub(crate) fn record_owned(
    diagnostics: &mut Option<Diagnostics>,
    phase: Phase,
    vm_id: &str,
    request_id: Option<&str>,
    message: &str,
) {
    let Some(handle) = diagnostics.as_mut() else {
        return;
    };
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), vm_id.to_owned());
    let event = VmEvent::host(
        phase,
        message.to_owned(),
        request_id.map(str::to_owned),
        context,
    );
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics record failed");
    }
}

/// Record one host-side lifecycle event with additional structured context.
pub(crate) fn record_context(
    diagnostics: &mut Option<Diagnostics>,
    phase: Phase,
    vm_id: &str,
    request_id: Option<&str>,
    message: &str,
    extra: impl IntoIterator<Item = (String, String)>,
) {
    let Some(handle) = diagnostics.as_mut() else {
        return;
    };
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), vm_id.to_owned());
    for (key, value) in extra {
        context.insert(key, value);
    }
    let event = VmEvent::host(
        phase,
        message.to_owned(),
        request_id.map(str::to_owned),
        context,
    );
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics context record failed");
    }
}

/// Record one request-scoped host-side wire protocol failure.
pub(crate) fn record_protocol_error(
    diagnostics: &mut Option<Diagnostics>,
    vm_id: &str,
    request_id: &str,
    stream_id: &str,
    error: &impl fmt::Display,
) {
    let Some(handle) = diagnostics.as_mut() else {
        return;
    };
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), vm_id.to_owned());
    context.insert("stream_id".to_owned(), stream_id.to_owned());
    context.insert("error".to_owned(), error.to_string());
    let event = VmEvent::host(
        Phase::Request,
        "protocol_error",
        Some(request_id.to_owned()),
        context,
    );
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics protocol-error record failed");
    }
}

/// Record typed stop/capture evidence.
pub(crate) fn record_stop_reason(
    diagnostics: &mut Option<Diagnostics>,
    vm_id: &str,
    request_id: Option<&str>,
    message: &str,
    reason: ExitReason,
) {
    let Some(handle) = diagnostics.as_mut() else {
        return;
    };
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), vm_id.to_owned());
    let event = VmEvent::host(
        Phase::Stop,
        message.to_owned(),
        request_id.map(str::to_owned),
        context,
    )
    .with_exit_reason(reason);
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics stop-reason record failed");
    }
}

/// Record a timed phase start/completion pair around a fallible operation.
pub(crate) fn phase_result<T, E, F>(
    diagnostics: &mut Option<Diagnostics>,
    phase: Phase,
    phase_name: &str,
    vm_id: &str,
    request_id: Option<&str>,
    f: F,
) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E>,
{
    record_phase_started(diagnostics.as_mut(), phase, phase_name, vm_id, request_id);
    let started = Instant::now();
    let result = f();
    let elapsed = started.elapsed();
    phase_event(phase_name, vm_id, elapsed);
    let outcome = match &result {
        Ok(_) => PhaseOutcome::Ok,
        Err(_) => PhaseOutcome::Err {
            class: std::any::type_name::<E>().to_owned(),
        },
    };
    record_phase_completed(
        diagnostics.as_mut(),
        phase,
        phase_name,
        vm_id,
        request_id,
        u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX),
        outcome,
    );
    result
}

fn record_phase_started(
    diagnostics: Option<&mut Diagnostics>,
    phase: Phase,
    phase_name: &str,
    vm_id: &str,
    request_id: Option<&str>,
) {
    let Some(handle) = diagnostics else {
        return;
    };
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), vm_id.to_owned());
    context.insert("phase_name".to_owned(), phase_name.to_owned());
    let event = VmEvent::phase_started(
        phase,
        "phase_started",
        request_id.map(str::to_owned),
        context,
    );
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics phase-start record failed");
    }
}

fn record_phase_completed(
    diagnostics: Option<&mut Diagnostics>,
    phase: Phase,
    phase_name: &str,
    vm_id: &str,
    request_id: Option<&str>,
    duration_us: u64,
    outcome: PhaseOutcome,
) {
    let Some(handle) = diagnostics else {
        return;
    };
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), vm_id.to_owned());
    context.insert("phase_name".to_owned(), phase_name.to_owned());
    let event = VmEvent::phase_completed(
        phase,
        "phase_completed",
        request_id.map(str::to_owned),
        context,
        duration_us,
        outcome,
    );
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics phase-complete record failed");
    }
}

/// Emit one phase timing event to stderr (no-op unless `M80_PHASE_TRACE=1`).
///
/// Each phase boundary in `launch::launch`, `RunningSandbox::exec`, and
/// `RunningSandbox::stop` emits one line on stderr in the format:
/// ```text
/// M80_PHASE name=stop_bounded vm_id=vm-1234 elapsed_us=12345
/// ```
/// The bench script (`scripts/bench-cold-launch.sh`) parses these into a
/// long-format CSV so per-phase contributions can be attributed without
/// re-running. No-op in production (env var unset).
pub(crate) fn phase_event(name: &str, vm_id: &str, elapsed: std::time::Duration) {
    if !std::env::var("M80_PHASE_TRACE").is_ok_and(|v| v == "1") {
        return;
    }
    eprintln!(
        "M80_PHASE name={} vm_id={} elapsed_us={}",
        name,
        vm_id,
        elapsed.as_micros()
    );
}

/// Run a closure and emit a phase event with its elapsed time. Returns
/// the closure's result so call sites read like the unwrapped call:
///
/// ```ignore
/// let storage = phase("phase_3_storage_prep", &vm_id, || {
///     phase_3_storage_prep(...)
/// })?;
/// ```
pub(crate) fn phase<T, F: FnOnce() -> T>(name: &str, vm_id: &str, f: F) -> T {
    let t = Instant::now();
    let out = f();
    phase_event(name, vm_id, t.elapsed());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_result_records_started_and_completed_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut diagnostics = Some(Diagnostics::open(dir.path()).unwrap());

        let result: Result<u32, std::io::Error> = phase_result(
            &mut diagnostics,
            Phase::Boot,
            "phase_test",
            "vm-test",
            Some("req-test"),
            || Ok(7),
        );
        assert_eq!(result.unwrap(), 7);

        drop(diagnostics);
        let text =
            std::fs::read_to_string(dir.path().join(m80_observability::DIAGNOSTICS_FILE_NAME))
                .unwrap();
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events[0]["event_kind"], "phase_started");
        assert_eq!(events[1]["event_kind"], "phase_completed");
        assert!(events[1]["duration_us"].is_u64());
        assert_eq!(events[1]["outcome"]["status"], "ok");
    }

    #[test]
    fn stop_reason_records_normal_force_and_snapshot_variants() {
        let dir = tempfile::tempdir().unwrap();
        let mut diagnostics = Some(Diagnostics::open(dir.path()).unwrap());
        record_stop_reason(
            &mut diagnostics,
            "vm-test",
            None,
            "stop complete",
            ExitReason::NormalStop,
        );
        record_stop_reason(
            &mut diagnostics,
            "vm-test",
            None,
            "force kill complete",
            ExitReason::ForceKill,
        );
        record_stop_reason(
            &mut diagnostics,
            "vm-test",
            None,
            "snapshot captured",
            ExitReason::SnapshotCapture,
        );

        drop(diagnostics);
        let text =
            std::fs::read_to_string(dir.path().join(m80_observability::DIAGNOSTICS_FILE_NAME))
                .unwrap();
        let reasons: Vec<String> = text
            .lines()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                value["exit_reason"]["reason"].as_str().unwrap().to_owned()
            })
            .collect();
        assert_eq!(reasons, ["normal_stop", "force_kill", "snapshot_capture"]);
    }

    #[test]
    fn protocol_error_records_request_and_stream_context() {
        let dir = tempfile::tempdir().unwrap();
        let mut diagnostics = Some(Diagnostics::open(dir.path()).unwrap());
        record_protocol_error(
            &mut diagnostics,
            "vm-test",
            "req-test",
            "exec_exit",
            &"disconnect before terminal frame in streaming exec",
        );

        drop(diagnostics);
        let text =
            std::fs::read_to_string(dir.path().join(m80_observability::DIAGNOSTICS_FILE_NAME))
                .unwrap();
        let event: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(event["request_id"], "req-test");
        assert_eq!(event["context"]["vm_id"], "vm-test");
        assert_eq!(event["context"]["stream_id"], "exec_exit");
        assert_eq!(
            event["context"]["error"],
            "disconnect before terminal frame in streaming exec"
        );
        assert_eq!(event["message"], "protocol_error");
    }
}
