//! Script-facing smoke tests for `m80 logs`.

mod common;

use common::m80;

#[test]
fn logs_request_id_filter_strict() {
    let run_root = tempfile::tempdir().unwrap();
    let run_dir = run_root.path().join("vm-a");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(
        run_dir.join("diagnostics.jsonl"),
        r#"{"schema_version":2,"timestamp_unix_ms":2000,"event_kind":"lifecycle","source_class":"host","phase":"Request","message":"exec request started","request_id":"req-1","context":{"vm_id":"vm-a"}}
{"schema_version":2,"timestamp_unix_ms":3000,"event_kind":"phase_completed","source_class":"host","phase":"Stop","message":"stop complete","request_id":"req-2"}
"#,
    )
    .unwrap();
    std::fs::write(
        run_dir.join("console.log"),
        "[1970-01-01T00:00:01Z] [Exec] [req-1] INFO guest line\n\
         [1970-01-01T00:00:04Z] [Exec] [req-2] INFO wrong request\n",
    )
    .unwrap();

    let output = m80()
        .env("M80_RUN_ROOT", run_root.path())
        .args(["logs", "vm-a", "--request-id", "req-1"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "logs filter should not need stderr diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("exec request started"), "{stdout}");
    assert!(stdout.contains("guest line"), "{stdout}");
    assert!(stdout.contains("req-1"), "{stdout}");
    assert!(
        !stdout.contains("req-2")
            && !stdout.contains("stop complete")
            && !stdout.contains("wrong request"),
        "request-id filter leaked nonmatching records:\n{stdout}"
    );
}
