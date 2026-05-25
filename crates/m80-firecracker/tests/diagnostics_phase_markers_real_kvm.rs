//! Real-KVM coverage for run-dir diagnostics emitted by launch, exec, and stop.

mod common;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};
use serde_json::Value;

use common::RunDirDumpGuard;

const REQUEST_ID: &str = "req-diagnostics-phase-e2e";
const EXEC_REQUEST_ID_PREFIX: &str = "req-diagnostics-phase-e2e-exec-";

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn diagnostics_phase_markers_emitted_on_exec() {
    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id("diag-e2e")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: Some(REQUEST_ID.to_owned()),
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let vm_id = running.vm_id().to_owned();

    let response = running
        .exec(ExecRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec /bin/true");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(response.exit_code, Some(0));

    let stopped = running.stop().expect("stop");
    let events = read_diagnostics(&run_dir);

    assert_event(
        &events,
        "phase_started",
        "StoragePrepare",
        Some("phase_3_storage_prep"),
        "phase_started",
        RequestIdMatch::Exact(REQUEST_ID),
        &vm_id,
    );
    assert_event(
        &events,
        "phase_completed",
        "Ready",
        Some("phase_12b_ready_accept"),
        "phase_completed",
        RequestIdMatch::Exact(REQUEST_ID),
        &vm_id,
    );
    assert_event(
        &events,
        "lifecycle",
        "Ready",
        None,
        "guestd ready",
        RequestIdMatch::Exact(REQUEST_ID),
        &vm_id,
    );
    assert_event(
        &events,
        "lifecycle",
        "Request",
        None,
        "exec request started",
        RequestIdMatch::Prefix(EXEC_REQUEST_ID_PREFIX),
        &vm_id,
    );
    assert_event(
        &events,
        "lifecycle",
        "Request",
        None,
        "exec request completed",
        RequestIdMatch::Prefix(EXEC_REQUEST_ID_PREFIX),
        &vm_id,
    );
    assert_event(
        &events,
        "lifecycle",
        "Stop",
        None,
        "stop complete",
        RequestIdMatch::Exact(REQUEST_ID),
        &vm_id,
    );

    assert!(
        events
            .iter()
            .filter(|event| event["request_id"] == REQUEST_ID)
            .count()
            >= 4,
        "expected request_id to correlate launch and stop events: {events:#?}"
    );
    assert!(
        events
            .iter()
            .filter(|event| {
                event["request_id"]
                    .as_str()
                    .is_some_and(|request_id| request_id.starts_with(EXEC_REQUEST_ID_PREFIX))
            })
            .count()
            >= 2,
        "expected derived exec request_id to correlate exec events: {events:#?}"
    );

    stopped.delete().expect("delete");
}

fn read_diagnostics(run_dir: &std::path::Path) -> Vec<Value> {
    let path = run_dir.join("diagnostics.jsonl");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .map(|line| serde_json::from_str(line).expect("diagnostics JSONL line parses"))
        .collect()
}

fn assert_event(
    events: &[Value],
    event_kind: &str,
    phase: &str,
    phase_name: Option<&str>,
    message: &str,
    request_id: RequestIdMatch<'_>,
    vm_id: &str,
) {
    assert!(
        events.iter().any(|event| {
            event["event_kind"] == event_kind
                && event["phase"] == phase
                && event["message"] == message
                && event["request_id"]
                    .as_str()
                    .is_some_and(|actual| request_id.matches(actual))
                && event["context"]["vm_id"] == vm_id
                && match phase_name {
                    Some(name) => event["context"]["phase_name"] == name,
                    None => true,
                }
        }),
        "missing diagnostics event kind={event_kind:?} phase={phase:?} phase_name={phase_name:?} message={message:?} request_id={request_id:?}; events={events:#?}"
    );
}

#[derive(Clone, Copy, Debug)]
enum RequestIdMatch<'a> {
    Exact(&'a str),
    Prefix(&'a str),
}

impl RequestIdMatch<'_> {
    fn matches(self, actual: &str) -> bool {
        match self {
            Self::Exact(expected) => actual == expected,
            Self::Prefix(expected) => actual.starts_with(expected),
        }
    }
}
