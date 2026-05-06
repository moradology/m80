use std::collections::BTreeMap;

use m80_observability::{Diagnostics, Phase, VmEvent, DIAGNOSTICS_FILE_NAME};

#[test]
fn diagnostics_log_jsonl_format_carries_request_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = Diagnostics::open(dir.path()).unwrap();
    let mut context = BTreeMap::new();
    context.insert("vm_id".to_owned(), "vm-diag".to_owned());

    diagnostics
        .record(&VmEvent::host(
            Phase::Request,
            "exec request started",
            Some("req-diag"),
            context,
        ))
        .unwrap();
    drop(diagnostics);

    let text = std::fs::read_to_string(dir.path().join(DIAGNOSTICS_FILE_NAME)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(parsed["schema_version"], 2);
    assert_eq!(parsed["phase"], "Request");
    assert_eq!(parsed["request_id"], "req-diag");
    assert_eq!(parsed["context"]["vm_id"], "vm-diag");
}

#[test]
fn diagnostics_disabled_handle_does_not_materialize_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = Diagnostics::disabled();

    diagnostics
        .record(&VmEvent::host(
            Phase::Boot,
            "ignored",
            None::<String>,
            BTreeMap::new(),
        ))
        .unwrap();

    assert!(!dir.path().join(DIAGNOSTICS_FILE_NAME).exists());
}
