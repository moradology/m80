//! Per-run diagnostics helpers.

use std::collections::BTreeMap;
use std::path::Path;

use m80_observability::{Diagnostics, Phase, VmEvent};

/// Open diagnostics for one VM run-dir.
pub(crate) fn open(run_dir: &Path, vm_id: &str, request_id: Option<&str>) -> Option<Diagnostics> {
    match Diagnostics::open(run_dir) {
        Ok(mut diagnostics) => {
            record(
                Some(&mut diagnostics),
                Phase::StartupScavenge,
                vm_id,
                request_id,
                "diagnostics opened",
            );
            Some(diagnostics)
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
pub(crate) fn record(
    diagnostics: Option<&mut Diagnostics>,
    phase: Phase,
    vm_id: &str,
    request_id: Option<&str>,
    message: &str,
) {
    let Some(handle) = diagnostics else {
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

/// Record through an optional owned handle.
pub(crate) fn record_owned(
    diagnostics: &mut Option<Diagnostics>,
    phase: Phase,
    vm_id: &str,
    request_id: Option<&str>,
    message: &str,
) {
    record(diagnostics.as_mut(), phase, vm_id, request_id, message);
}
