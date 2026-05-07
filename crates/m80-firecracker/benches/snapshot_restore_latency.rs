//! Real-KVM snapshot-restore ready-latency probe.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig, SnapshotPaths,
    FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};

fn main() {
    let n = env_usize("N", 50);
    let load = std::env::var("M80_SNAPSHOT_BENCH_LOAD").unwrap_or_else(|_| "idle".into());
    let output = std::env::var_os("M80_SNAPSHOT_BENCH_OUTPUT").map(PathBuf::from);
    let started_at = unix_timestamp();
    let discovery = m80_preflight::run().expect("preflight");
    let snapshot_dir = discovery
        .run_root
        .join(format!("snapshot-restore-bench-{}", std::process::id()));
    let paths = snapshot_paths(&snapshot_dir);

    let golden_backend = make_backend(discovery.clone(), 1);
    let golden = golden_backend
        .admit(sandbox_config(format!("sbg-{}", std::process::id())))
        .expect("admit golden");
    let mut running = golden.launch().expect("launch golden");
    running.capture(paths.clone()).expect("capture golden");
    running
        .force_kill()
        .expect("force-kill golden")
        .delete()
        .expect("delete golden");

    let mut stress = if load == "loaded" {
        Some(start_stress())
    } else {
        None
    };

    let restore_backend = make_backend(discovery.clone(), 1);
    let mut samples_us = Vec::with_capacity(n);
    for i in 0..n {
        let sandbox = restore_backend
            .admit(sandbox_config(format!("sbr-{}-{i}", std::process::id())))
            .expect("admit restore");
        let t = Instant::now();
        let restored = sandbox
            .launch_from_snapshot(paths.clone(), &discovery)
            .expect("launch_from_snapshot");
        samples_us.push(t.elapsed().as_micros() as u64);
        restored
            .stop()
            .expect("stop restored")
            .delete()
            .expect("delete restored");
    }

    if let Some(child) = stress.as_mut() {
        stop_stress(child);
    }
    let _ = fs::remove_dir_all(&snapshot_dir);

    let stats = Stats::from_samples(&samples_us);
    let json = render_json(started_at, &load, n, stats, &samples_us);
    println!("{json}");
    eprintln!(
        "snapshot-restore latency: load={load} n={n} p50={}us p95={}us max={}us mean={}us",
        stats.p50_us, stats.p95_us, stats.max_us, stats.mean_us
    );
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create output parent");
        }
        fs::write(path, format!("{json}\n")).expect("write output");
    }
}

fn make_backend(discovery: m80_preflight::Discovery, max_concurrent_vms: u32) -> Arc<Backend> {
    let run_root = discovery.run_root.clone();
    Arc::new(
        Backend::new(BackendConfig {
            discovery,
            max_concurrent_vms,
            run_root,
            jail_uid: env_u32("M80_JAIL_UID", 3000),
            jail_gid: env_u32("M80_JAIL_GID", 3000),
            cgroup_mode: CgroupMode::Disabled,
        })
        .expect("Backend::new"),
    )
}

fn sandbox_config(vm_id: String) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(FIRST_LINE_MEM_SIZE_MIB),
        boot_args: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        idle_timeout: None,
        daemonize: false,
        request_id: None,
    }
}

fn snapshot_paths(dir: &Path) -> SnapshotPaths {
    fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

#[derive(Clone, Copy)]
struct Stats {
    p50_us: u64,
    p95_us: u64,
    max_us: u64,
    mean_us: u64,
}

impl Stats {
    fn from_samples(samples: &[u64]) -> Self {
        assert!(!samples.is_empty(), "at least one sample is required");
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let sum: u64 = sorted.iter().sum();
        Stats {
            p50_us: percentile(&sorted, 50),
            p95_us: percentile(&sorted, 95),
            max_us: sorted[sorted.len() - 1],
            mean_us: sum / sorted.len() as u64,
        }
    }
}

fn percentile(sorted: &[u64], pct: usize) -> u64 {
    let idx = sorted
        .len()
        .saturating_mul(pct)
        .checked_div(100)
        .unwrap_or(0)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

fn render_json(
    started_at_unix: u64,
    load: &str,
    n: usize,
    stats: Stats,
    samples_us: &[u64],
) -> String {
    let samples = samples_us
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"bench\": \"snapshot_restore_latency\",\n",
            "  \"load\": \"{load}\",\n",
            "  \"started_at_unix\": {started_at_unix},\n",
            "  \"n\": {n},\n",
            "  \"unit\": \"us\",\n",
            "  \"p50_us\": {p50},\n",
            "  \"p95_us\": {p95},\n",
            "  \"max_us\": {max},\n",
            "  \"mean_us\": {mean},\n",
            "  \"samples_us\": [{samples}]\n",
            "}}"
        ),
        load = load,
        started_at_unix = started_at_unix,
        n = n,
        p50 = stats.p50_us,
        p95 = stats.p95_us,
        max = stats.max_us,
        mean = stats.mean_us,
        samples = samples
    )
}

fn start_stress() -> Child {
    let procs = std::env::var("STRESS_PROCS").unwrap_or_else(|_| {
        std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "1".into())
    });
    Command::new("stress-ng")
        .arg("--cpu")
        .arg(procs)
        .arg("--quiet")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start stress-ng")
}

fn stop_stress(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
