//! Real-KVM coverage for the `NetworkPolicy::AllowOutbound` egress promise.

mod common;


use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, ExecChunk, NetworkPolicy, SandboxConfig,
};
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

fn launch_outbound_vm() -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id("outbound-egress-e2e")),
            workspace: None,
            network: NetworkPolicy::AllowOutbound {
                exceptions: Vec::new(),
            },
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpu_template: None,
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let running = sandbox.launch().expect("launch outbound");
    let run_dir = running.run_dir().to_owned();
    (running, run_dir)
}

fn shell_request(script: &str) -> ExecRequest {
    ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(10_000),
        streaming: false,
    }
}

fn external_network_enabled() -> bool {
    std::env::var_os("M80_RUN_EXTERNAL_NETWORK_E2E").is_some()
}

fn run_outbound_probe(script: &str) -> m80_proto::ExecResponse {
    let (mut running, run_dir) = launch_outbound_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let response = running
        .exec_streaming(shell_request(script), |chunk| {
            match chunk {
                ExecChunk::Stdout { bytes, .. } => stdout.extend(bytes),
                ExecChunk::Stderr { bytes, .. } => stderr.extend(bytes),
            }
            Ok(())
        })
        .map(|exit| m80_proto::ExecResponse {
            status: exit.status,
            exit_code: exit.exit_code,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            truncated: exit.truncated.then_some(true),
            timing: exit.timing,
        });
    let stopped = running.stop().expect("stop");
    match response {
        Ok(response) => {
            if response.status != ExecStatus::Completed || response.exit_code != Some(0) {
                let stdout = String::from_utf8_lossy(&response.stdout);
                let stderr = String::from_utf8_lossy(&response.stderr);
                let preserved = stopped.preserve_for_triage().expect("preserve run dir");
                panic!(
                    "exec outbound probe failed: status={:?}; exit={:?}; stdout={stdout:?}; stderr={stderr:?}; preserved run dir={}",
                    response.status,
                    response.exit_code,
                    preserved.display()
                );
            }
            stopped.delete().expect("delete");
            response
        }
        Err(err) => {
            let preserved = stopped.preserve_for_triage().expect("preserve run dir");
            let stdout = String::from_utf8_lossy(&stdout);
            let stderr = String::from_utf8_lossy(&stderr);
            panic!(
                "exec outbound probe: {err:?}; stdout={stdout:?}; stderr={stderr:?}; preserved run dir={}",
                preserved.display()
            );
        }
    }
}

#[test]
#[ignore = "requires KVM host, CAP_NET_ADMIN, and opt-in external network"]
fn allow_outbound_resolves_external_dns() {
    if !external_network_enabled() {
        eprintln!("skipping: set M80_RUN_EXTERNAL_NETWORK_E2E=1 to run external-network probe");
        return;
    }

    let response = run_outbound_probe("/bin/busybox timeout 4 /bin/busybox nslookup example.com");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "AllowOutbound must resolve external DNS; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
}

#[test]
#[ignore = "requires KVM host, CAP_NET_ADMIN, and opt-in external network"]
fn allow_outbound_reaches_external_http() {
    if !external_network_enabled() {
        eprintln!("skipping: set M80_RUN_EXTERNAL_NETWORK_E2E=1 to run external-network probe");
        return;
    }

    let response = run_outbound_probe(
        "/bin/busybox wget -T 4 -O /tmp/m80-http.out http://httpforever.com && /bin/busybox grep -q 'HTTP Forever' /tmp/m80-http.out",
    );

    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "AllowOutbound must reach external HTTP; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
}
