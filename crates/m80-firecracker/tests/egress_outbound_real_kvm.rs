//! Real-KVM coverage for the `NetworkPolicy::AllowOutbound` egress promise.

mod common;

use std::net::Ipv4Addr;
use std::path::Path;

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, ExecChunk, NetworkPolicy, SandboxConfig,
};
use m80_proto::{ExecRequest, ExecStatus};
use serde::Deserialize;

use common::RunDirDumpGuard;

fn outbound_backend(max_concurrent_vms: u32) -> std::sync::Arc<Backend> {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    std::sync::Arc::new(Backend::new(config).expect("Backend::new"))
}

fn launch_outbound_vm() -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let backend = outbound_backend(1);
    launch_outbound_vm_on_backend(backend, "outbound-egress-e2e")
}

fn launch_outbound_vm_on_backend(
    backend: std::sync::Arc<Backend>,
    vm_id_prefix: &str,
) -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id(vm_id_prefix)),
            workspace: None,
            network: NetworkPolicy::AllowOutbound {
                exceptions: Vec::new(),
            },
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            cpuset_cpus: None,
            cpu_template: None,
            drive_cache_type: None,
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
    let response = run_outbound_probe_response(script);
    if response.status != ExecStatus::Completed || response.exit_code != Some(0) {
        panic!(
            "exec outbound probe failed: status={:?}; exit={:?}; stdout={:?}; stderr={:?}",
            response.status,
            response.exit_code,
            String::from_utf8_lossy(&response.stdout),
            String::from_utf8_lossy(&response.stderr),
        );
    }
    response
}

fn run_outbound_probe_response(script: &str) -> m80_proto::ExecResponse {
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

#[derive(Deserialize)]
struct GuestIpNetworkState {
    guest_ipv4: Ipv4Addr,
}

fn guest_ipv4_from_network_state(run_dir: &Path) -> Ipv4Addr {
    let path = run_dir.join(m80_net_outbound::NETWORK_STATE_FILE);
    let raw = std::fs::read_to_string(&path).expect("read network-state.json");
    serde_json::from_str::<GuestIpNetworkState>(&raw)
        .expect("parse network-state.json")
        .guest_ipv4
}

#[test]
#[ignore = "requires KVM host and CAP_NET_ADMIN"]
fn allow_outbound_rejects_peer_guest_ipv4_on_shared_bridge() {
    let backend = outbound_backend(2);
    let (mut first, first_run_dir) =
        launch_outbound_vm_on_backend(backend.clone(), "outbound-peer-a");
    let (second, second_run_dir) = launch_outbound_vm_on_backend(backend, "outbound-peer-b");
    let _first_dump = RunDirDumpGuard::new(first_run_dir.clone());
    let _second_dump = RunDirDumpGuard::new(second_run_dir.clone());
    let second_guest_ipv4 = guest_ipv4_from_network_state(&second_run_dir);

    let response = first
        .exec(shell_request(&format!(
            "/bin/busybox timeout 4 /bin/busybox ping -c 1 -W 2 {}",
            second_guest_ipv4
        )))
        .expect("exec peer ping probe");

    let first_stopped = first.stop().expect("stop first");
    let second_stopped = second.stop().expect("stop second");
    first_stopped.delete().expect("delete first");
    second_stopped.delete().expect("delete second");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_ne!(
        response.exit_code,
        Some(0),
        "AllowOutbound must reject direct guest-to-guest IPv4 on the shared bridge; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
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
fn allow_outbound_rejects_external_icmp() {
    if !external_network_enabled() {
        eprintln!("skipping: set M80_RUN_EXTERNAL_NETWORK_E2E=1 to run external-network probe");
        return;
    }

    let response =
        run_outbound_probe_response("/bin/busybox timeout 4 /bin/busybox ping -c 1 -W 2 1.1.1.1");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_ne!(
        response.exit_code,
        Some(0),
        "AllowOutbound must reject external ICMP; stdout={:?}; stderr={:?}",
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
