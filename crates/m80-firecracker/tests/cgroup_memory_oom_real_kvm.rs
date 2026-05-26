//! Real-KVM coverage that unified-v2 `memory.max` is enforced, not just
//! written.

mod common;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::ExecRequest;

use common::RunDirDumpGuard;

// vm_id must stay under ~22 chars so the AF_UNIX socket path
// `<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock` fits the
// 107-byte kernel cap (vm_id appears twice in the jail layout).

fn memhog_request(mib: u32) -> ExecRequest {
    ExecRequest {
        program: "/tmp/m80-memhog".into(),
        args: vec![mib.to_string()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(45_000),
        streaming: false,
    }
}

#[test]
#[ignore = "requires-kvm requires-root requires-cgroup-v2 requires-artifacts"]
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

    let memhog = compile_memhog_binary();
    running
        .upload_file_chunked(
            "/tmp/m80-memhog",
            Some(0o755),
            Cursor::new(memhog),
            1024 * 1024,
        )
        .expect("upload memhog helper");

    let result = running.exec(memhog_request(1_800));

    let after = wait_for_memory_event(&cgroup, &before);
    assert!(
        event_increased(&before, &after, "oom_kill") || event_increased(&before, &after, "oom"),
        "cgroup memory.events must record OOM enforcement; before={before:?} after={after:?} exec_result={result:?}"
    );

    let stopped = running.force_kill().expect("force-kill after OOM probe");
    stopped.delete().expect("delete");
}

fn compile_memhog_binary() -> Vec<u8> {
    let build_dir = tempfile::tempdir().expect("tempdir for memhog build");
    let source = build_dir.path().join("m80-memhog.c");
    let output = build_dir.path().join("m80-memhog");
    std::fs::write(
        &source,
        r#"
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    unsigned long mib = 1800;
    if (argc > 1) {
        char *end = NULL;
        errno = 0;
        mib = strtoul(argv[1], &end, 10);
        if (errno != 0 || end == argv[1] || *end != '\0' || mib == 0) {
            fprintf(stderr, "invalid MiB argument\n");
            return 2;
        }
    }

    size_t bytes = (size_t)mib * 1024u * 1024u;
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    if (page == 0) {
        page = 4096;
    }

    unsigned char *buf = malloc(bytes);
    if (buf == NULL) {
        fprintf(stderr, "malloc(%zu) failed: %s\n", bytes, strerror(errno));
        return 3;
    }

    for (size_t offset = 0; offset < bytes; offset += page) {
        buf[offset] = (unsigned char)(offset / page);
    }
    buf[bytes - 1] = 1;
    printf("touched %lu MiB\n", mib);
    fflush(stdout);
    sleep(5);
    return 0;
}
"#,
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", source.display()));

    let status = Command::new("cc")
        .arg("-std=c11")
        .arg("-O2")
        .arg("-static")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-o")
        .arg(&output)
        .arg(&source)
        .status()
        .unwrap_or_else(|e| panic!("spawn cc for {}: {e}", source.display()));
    assert!(
        status.success(),
        "compile {} with cc -static failed: {status}",
        source.display()
    );

    std::fs::read(&output).unwrap_or_else(|e| panic!("read {}: {e}", output.display()))
}

fn wait_for_memory_event(cgroup: &Path, before: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let after = read_memory_events(cgroup);
        if event_increased(before, &after, "oom_kill") || event_increased(before, &after, "oom") {
            return after;
        }
        if Instant::now() >= deadline {
            return after;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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
