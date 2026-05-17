//! Real-KVM coverage for the `NetworkPolicy::NoEgress` isolation promise.

mod common;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

fn launch_no_egress_vm() -> (m80_firecracker::RunningSandbox, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
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
            vm_id: Some(common::unique_vm_id("no-egress-e2e")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
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
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let running = sandbox.launch().expect("launch");
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
        timeout_ms: Some(7_000),
        streaming: false,
    }
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn no_egress_firecracker_runs_in_private_netns() {
    let (running, run_dir) = launch_no_egress_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let firecracker_pid = firecracker_pid(&run_dir);

    let host_netns = std::fs::read_link("/proc/self/ns/net").expect("host netns");
    let firecracker_netns =
        std::fs::read_link(format!("/proc/{firecracker_pid}/ns/net")).expect("firecracker netns");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");

    assert_ne!(
        firecracker_netns, host_netns,
        "NoEgress Firecracker must not run in the host network namespace"
    );
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn no_egress_blocks_external_ip_traffic() {
    let (mut running, run_dir) = launch_no_egress_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let response = running
        .exec(shell_request("wget -T 2 -O - http://1.1.1.1"))
        .expect("exec external ip probe");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_ne!(
        response.exit_code,
        Some(0),
        "NoEgress must block direct external IP traffic; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

fn firecracker_pid(run_dir: &std::path::Path) -> u32 {
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
#[ignore = "requires KVM host with real Firecracker binary"]
fn no_egress_blocks_external_dns_resolution() {
    let (mut running, run_dir) = launch_no_egress_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let response = running
        .exec(shell_request("nslookup example.com"))
        .expect("exec dns probe");

    assert_eq!(response.status, ExecStatus::Completed);
    assert_ne!(
        response.exit_code,
        Some(0),
        "NoEgress must block external DNS resolution; stdout={:?}; stderr={:?}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
