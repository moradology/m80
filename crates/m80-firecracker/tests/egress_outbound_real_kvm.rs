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

const CAP_NET_ADMIN_MASK: u64 = 1u64 << 12;

fn outbound_backend(max_concurrent_vms: u32) -> std::sync::Arc<Backend> {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(max_concurrent_vms)
        .run_root(run_root)
        .jail_uid(env_u32("M80_JAIL_UID", 3000))
        .jail_gid(env_u32("M80_JAIL_GID", 3000))
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    std::sync::Arc::new(Backend::new(config).expect("Backend::new"))
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(default)
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
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
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

fn assert_backend_thread_lacks_cap_net_admin() {
    let status = std::fs::read_to_string("/proc/thread-self/status").expect("status");
    for field in ["CapEff", "CapPrm", "CapBnd"] {
        let value = status_hex_value(&status, field);
        assert_eq!(
            value & CAP_NET_ADMIN_MASK,
            0,
            "{field} still contains CAP_NET_ADMIN"
        );
    }
}

fn status_hex_value(status: &str, field: &str) -> u64 {
    let prefix = format!("{field}:\t");
    let raw = status
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("{field} missing from /proc/thread-self/status"));
    u64::from_str_radix(raw.trim(), 16).expect("hex capability field")
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

fn firecracker_pid(run_dir: &Path) -> u32 {
    let state_path = run_dir.join("jailer-state.json");
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display())),
    )
    .expect("jailer-state.json parses");
    state["firecracker_pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("firecracker_pid missing from {}", state_path.display()))
        as u32
}

#[test]
#[ignore = "requires KVM host and CAP_NET_ADMIN"]
fn allow_outbound_firecracker_runs_in_private_netns() {
    let (running, run_dir) = launch_outbound_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let firecracker_pid = firecracker_pid(&run_dir);

    let host_netns = std::fs::read_link("/proc/self/ns/net").expect("host netns");
    let firecracker_netns =
        std::fs::read_link(format!("/proc/{firecracker_pid}/ns/net")).expect("firecracker netns");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");

    assert_ne!(
        firecracker_netns, host_netns,
        "AllowOutbound Firecracker must not run in the host network namespace"
    );
}

#[test]
#[ignore = "requires KVM host, CAP_NET_ADMIN, and opt-in external network"]
fn allow_outbound_reaches_external_http_by_ip() {
    if !external_network_enabled() {
        eprintln!("skipping: set M80_RUN_EXTERNAL_NETWORK_E2E=1 to run external-network probe");
        return;
    }

    let response =
        run_outbound_probe("/bin/busybox wget -T 4 -O /tmp/m80-http-ip.out http://146.190.62.39");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "AllowOutbound must reach external HTTP by IP; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
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
#[ignore = "requires KVM host, CAP_NET_ADMIN at process start, and opt-in external network"]
fn allow_outbound_survives_parent_cap_net_admin_drop() {
    if !external_network_enabled() {
        eprintln!("skipping: set M80_RUN_EXTERNAL_NETWORK_E2E=1 to run external-network probe");
        return;
    }

    let backend = outbound_backend(1);
    assert_backend_thread_lacks_cap_net_admin();

    let (mut running, run_dir) = launch_outbound_vm_on_backend(backend.clone(), "capdel");
    let _delete_dump = RunDirDumpGuard::new(run_dir.clone());
    let response = running
        .exec(shell_request(
            "/bin/busybox timeout 8 /bin/busybox nslookup example.com >/tmp/m80-dns.out && /bin/busybox wget -T 4 -O /tmp/m80-http-ip.out http://146.190.62.39",
        ))
        .expect("exec external DNS and HTTP probe");
    let stopped = running.stop().expect("stop delete probe");
    stopped.delete().expect("delete outbound VM");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "AllowOutbound must keep DNS and HTTP-by-IP working after backend-thread CAP_NET_ADMIN drop; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
    assert!(
        !run_dir.exists(),
        "delete must remove outbound run-dir {}",
        run_dir.display()
    );

    let (running, stale_run_dir) = launch_outbound_vm_on_backend(backend.clone(), "capstale");
    let _stale_dump = RunDirDumpGuard::new(stale_run_dir.clone());
    let stopped = running.stop().expect("stop stale probe");
    drop(stopped);
    backend
        .recover_stale_run_root(true)
        .expect("recover stale outbound run-dir after parent cap drop");
    assert!(
        !stale_run_dir.exists(),
        "stale recovery must remove outbound run-dir {}",
        stale_run_dir.display()
    );
    assert_backend_thread_lacks_cap_net_admin();
}

#[test]
#[ignore = "requires KVM host, CAP_NET_ADMIN, and opt-in external network"]
fn allow_outbound_resolves_external_dns() {
    if !external_network_enabled() {
        eprintln!("skipping: set M80_RUN_EXTERNAL_NETWORK_E2E=1 to run external-network probe");
        return;
    }

    let response = run_outbound_probe("/bin/busybox timeout 8 /bin/busybox nslookup example.com");

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
