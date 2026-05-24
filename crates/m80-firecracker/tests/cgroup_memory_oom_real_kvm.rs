//! Real-KVM coverage that unified-v2 `memory.max` is enforced, not just
//! written.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::ExecRequest;

use common::RunDirDumpGuard;

// vm_id must stay under ~22 chars so the AF_UNIX socket path
// `<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock` fits the
// 107-byte kernel cap (vm_id appears twice in the jail layout).

fn shell_request(script: &str) -> ExecRequest {
    ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(45_000),
        streaming: false,
    }
}

#[test]
#[ignore = "requires root, writable cgroup v2, KVM, and real Firecracker binary"]
fn cgroup_memory_limit_oom_kills_workload() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable cgroup-v2 host");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::UnifiedV2)
        .build();
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let vm_id = common::unique_vm_id("cgr-oom");
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(vm_id),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(2048),
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            daemonize: false,
            request_id: Some("req-cgroup-memory-oom".to_owned()),
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch with cgroup mode");
    let run_dir = running.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());
    let cgroup = read_cgroup_path(&run_dir);
    let before = read_memory_events(&cgroup);

    let memory_max = std::fs::read_to_string(cgroup.join("memory.max")).expect("memory.max");
    assert_eq!(
        memory_max.trim(),
        m80_cgroup::Limits::preset()
            .memory_max
            .expect("default memory.max")
            .to_string(),
        "firecracker must apply the default host memory cap before workload"
    );

    let result = running.exec(shell_request(
        "set -eu\n\
         mkdir -p /tmp/m80-memhog\n\
         mount -t tmpfs -o size=1900m tmpfs /tmp/m80-memhog\n\
         dd if=/dev/zero of=/tmp/m80-memhog/blob bs=1M count=1800",
    ));

    let after = read_memory_events(&cgroup);
    assert!(
        event_increased(&before, &after, "oom_kill") || event_increased(&before, &after, "oom"),
        "cgroup memory.events must record OOM enforcement; before={before:?} after={after:?} exec_result={result:?}"
    );

    let stopped = running.force_kill().expect("force-kill after OOM probe");
    stopped.delete().expect("delete");
}

fn read_cgroup_path(run_dir: &Path) -> PathBuf {
    let path = run_dir.join("cgroup-path.txt");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    PathBuf::from(text.trim())
}

fn read_memory_events(cgroup: &Path) -> BTreeMap<String, u64> {
    let path = cgroup.join("memory.events");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .map(|line| {
            let (key, value) = line
                .split_once(' ')
                .unwrap_or_else(|| panic!("invalid memory.events line: {line:?}"));
            let value = value
                .parse::<u64>()
                .unwrap_or_else(|e| panic!("invalid memory.events value {value:?}: {e}"));
            (key.to_owned(), value)
        })
        .collect()
}

fn event_increased(
    before: &BTreeMap<String, u64>,
    after: &BTreeMap<String, u64>,
    key: &str,
) -> bool {
    after.get(key).copied().unwrap_or(0) > before.get(key).copied().unwrap_or(0)
}
