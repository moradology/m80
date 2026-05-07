//! Per-run diagnostics helpers.

use std::collections::BTreeMap;
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
    crate::timing::phase_event(phase_name, vm_id, elapsed);
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
        elapsed.as_micros() as u64,
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
        format!("{phase_name} started"),
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
        format!("{phase_name} completed"),
        request_id.map(str::to_owned),
        context,
        duration_us,
        outcome,
    );
    if let Err(e) = handle.record(&event) {
        tracing::warn!(vm_id, err = %e, "diagnostics phase-complete record failed");
    }
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
}
