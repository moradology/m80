//! Real-KVM smoke coverage for the guestd seccomp cutover.

use std::sync::mpsc;

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, ExecChunk, NetworkPolicy, SandboxConfig,
};
use m80_proto::{ExecRequest, ExecStatus, PtyRequest, PtySize};

use crate::common::RunDirDumpGuard;

fn launch_vm() -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(jail_id_from_env("M80_JAIL_UID"))
        .jail_gid(jail_id_from_env("M80_JAIL_GID"))
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(crate::common::unique_vm_id("guestd-seccomp")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
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
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_owned();
    (running, run_dir)
}

fn jail_id_from_env(name: &str) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(3000)
}

fn cat_request(path: &str) -> ExecRequest {
    ExecRequest {
        program: "/bin/cat".into(),
        args: vec![path.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn seccomp_probe_request() -> ExecRequest {
    ExecRequest {
        program: "/m80-guestd".into(),
        args: vec![
            "--m80-seccomp-probe".into(),
            "workload".into(),
            "deny-unshare".into(),
        ],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn pty_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: None,
        pixel_height: None,
    }
}

fn pty_cat_status_request() -> PtyRequest {
    PtyRequest {
        program: "/bin/cat".into(),
        args: vec!["/proc/thread-self/status".into()],
        cwd: None,
        env: None,
        timeout_ms: Some(5_000),
        size: pty_size(),
    }
}

fn assert_completed(response: m80_proto::ExecResponse, context: &str) -> Vec<u8> {
    assert_eq!(response.status, ExecStatus::Completed, "{context}");
    assert_eq!(response.exit_code, Some(0), "{context}");
    assert!(
        response.stderr.is_empty(),
        "{context} stderr: {}",
        String::from_utf8_lossy(&response.stderr)
    );
    response.stdout
}

fn assert_daemon_seccomp(status: &str) {
    assert_status_value(status, "Name", "m80-guestd");
    assert_status_value(status, "Seccomp", "2");
}

fn assert_workload_status(status: &str, context: &str) {
    assert_status_value_with_context(status, "Uid", "1000\t1000\t1000\t1000", context);
    assert_status_value_with_context(status, "Gid", "1000\t1000\t1000\t1000", context);
    assert_status_value_with_context(status, "NoNewPrivs", "1", context);
    assert_status_value_with_context(status, "Seccomp", "2", context);
    for field in ["CapInh", "CapPrm", "CapEff", "CapBnd"] {
        assert_status_value_with_context(status, field, "0000000000000000", context);
    }
}

fn assert_status_value(status: &str, field: &str, expected: &str) {
    assert_status_value_with_context(status, field, expected, field);
}

fn assert_status_value_with_context(status: &str, field: &str, expected: &str, context: &str) {
    let prefix = format!("{field}:");
    let actual = status
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .unwrap_or_else(|| panic!("{context}: missing {field} in status:\n{status}"));
    assert_eq!(actual, expected, "{context}: unexpected {field}");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary and current m80-guestd image"]
fn real_kvm_guestd_seccomp_preserves_exec_paths() {
    let (mut running, run_dir) = launch_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let daemon_status = assert_completed(
        running.exec(cat_request("/proc/1/status")).unwrap(),
        "daemon status",
    );
    assert_daemon_seccomp(&String::from_utf8(daemon_status).expect("daemon status utf8"));

    let buffered_status = assert_completed(
        running
            .exec(cat_request("/proc/thread-self/status"))
            .unwrap(),
        "buffered workload status",
    );
    assert_workload_status(
        &String::from_utf8(buffered_status).expect("buffered status utf8"),
        "buffered workload status",
    );

    let mut streaming_status = Vec::new();
    let streaming_exit = running
        .exec_streaming(cat_request("/proc/thread-self/status"), |chunk| {
            if let ExecChunk::Stdout { bytes, .. } = chunk {
                streaming_status.extend(bytes);
            }
            Ok(())
        })
        .expect("streaming workload status");
    assert_eq!(streaming_exit.status, ExecStatus::Completed);
    assert_eq!(streaming_exit.exit_code, Some(0));
    assert_workload_status(
        &String::from_utf8(streaming_status).expect("streaming status utf8"),
        "streaming workload status",
    );

    let (_tx, rx) = mpsc::channel();
    let mut pty_status = Vec::new();
    let pty_exit = running
        .exec_pty(pty_cat_status_request(), rx, |chunk| {
            pty_status.extend_from_slice(&chunk.bytes);
            Ok(())
        })
        .expect("pty workload status");
    assert_eq!(pty_exit.status, ExecStatus::Completed);
    assert_eq!(pty_exit.exit_code, Some(0));
    assert_workload_status(
        &String::from_utf8(pty_status).expect("pty status utf8"),
        "pty workload status",
    );

    let denied = assert_completed(
        running.exec(seccomp_probe_request()).unwrap(),
        "workload denied syscall probe",
    );
    assert_eq!(
        String::from_utf8(denied).expect("denied probe utf8").trim(),
        "unshare=blocked"
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
