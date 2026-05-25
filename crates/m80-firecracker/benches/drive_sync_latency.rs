//! Real-KVM writable-drive sync latency probe.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    Backend, BackendConfig, CacheType, CgroupMode, NetworkPolicy, SandboxConfig,
};
use m80_proto::{ExecRequest, ExecStatus};

fn main() {
    let n = env_usize("N", 10);
    let syncs = env_usize("M80_DRIVE_SYNC_COUNT", 100);
    let cache_type = cache_type_from_env();
    let output = std::env::var_os("M80_DRIVE_SYNC_BENCH_OUTPUT").map(PathBuf::from);
    let started_at = unix_timestamp();

    let discovery = m80_preflight::run().expect("preflight");
    let run_root = discovery.run_root.clone();
    let backend = Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(1)
                .run_root(run_root)
                .jail_uid(env_u32("M80_JAIL_UID", 3000))
                .jail_gid(env_u32("M80_JAIL_GID", 3000))
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    );

    let vm_id = format!("ds{}{}", cache_type.short_label(), std::process::id());
    let sandbox = backend
        .admit(sandbox_config(vm_id, cache_type))
        .expect("admit");
    let launch_started = Instant::now();
    let mut running = sandbox.launch().expect("launch");
    let launch_ms = launch_started.elapsed().as_millis() as u64;

    let warmup = running
        .exec(sync_request("warmup", 1))
        .expect("warmup exec");
    assert_eq!(warmup.status, ExecStatus::Completed);
    assert_eq!(warmup.exit_code, Some(0), "warmup failed");

    let mut samples_ms = Vec::with_capacity(n);
    for i in 0..n {
        let t = Instant::now();
        let resp = running
            .exec(sync_request(&format!("sample-{i}"), syncs))
            .expect("sync-heavy exec");
        let elapsed = t.elapsed();
        assert_eq!(resp.status, ExecStatus::Completed);
        assert_eq!(resp.exit_code, Some(0), "sync-heavy exec {i} failed");
        samples_ms.push(elapsed.as_millis() as u64);
    }

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");

    let stats = Stats::from_samples(&samples_ms);
    let json = render_json(
        started_at,
        n,
        syncs,
        cache_type,
        launch_ms,
        stats,
        &samples_ms,
    );
    println!("{json}");
    eprintln!(
        "drive-sync latency: cache_type={} n={n} syncs={syncs} p50={}ms p95={}ms max={}ms mean={}ms",
        cache_type.label(),
        stats.p50_ms,
        stats.p95_ms,
        stats.max_ms,
        stats.mean_ms
    );
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create output parent");
        }
        fs::write(path, format!("{json}\n")).expect("write output");
    }
}

fn sandbox_config(vm_id: String, cache_type: CacheMode) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        drive_cache_type: cache_type.to_cache_type(),
        boot_args: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        overlay_clone_mode: Default::default(),
        idle_timeout: None,
        max_lifetime: None,
        daemonize: false,
        request_id: None,
        preallocated_drive_slots: 0,
        pmem_layers: Vec::new(),
        one_shot: false,
    }
}

fn sync_request(label: &str, syncs: usize) -> ExecRequest {
    let script = format!(
        "set -eu; d=/tmp/m80-drive-sync-{label}; rm -rf \"$d\"; mkdir \"$d\"; i=0; while [ \"$i\" -lt {syncs} ]; do dd if=/dev/zero of=\"$d/file-$i\" bs=4096 count=1 conv=fsync status=none; sync; i=$((i+1)); done"
    );
    ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(120_000),
        streaming: false,
    }
}

#[derive(Clone, Copy)]
enum CacheMode {
    Unsafe,
    Writeback,
}

impl CacheMode {
    fn label(self) -> &'static str {
        match self {
            CacheMode::Unsafe => "Unsafe",
            CacheMode::Writeback => "Writeback",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            CacheMode::Unsafe => "u",
            CacheMode::Writeback => "w",
        }
    }

    fn to_cache_type(self) -> Option<CacheType> {
        Some(match self {
            CacheMode::Unsafe => CacheType::Unsafe,
            CacheMode::Writeback => CacheType::Writeback,
        })
    }
}

fn cache_type_from_env() -> CacheMode {
    match std::env::var("M80_DRIVE_CACHE_TYPE").as_deref() {
        Ok("Unsafe") => CacheMode::Unsafe,
        Ok("Writeback") => CacheMode::Writeback,
        Ok(other) => panic!("M80_DRIVE_CACHE_TYPE must be Unsafe or Writeback, got {other:?}"),
        Err(_) => CacheMode::Unsafe,
    }
}

#[derive(Clone, Copy)]
struct Stats {
    p50_ms: u64,
    p95_ms: u64,
    max_ms: u64,
    mean_ms: u64,
}

impl Stats {
    fn from_samples(samples: &[u64]) -> Self {
        assert!(!samples.is_empty(), "at least one sample is required");
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let sum: u64 = sorted.iter().sum();
        Stats {
            p50_ms: percentile(&sorted, 50),
            p95_ms: percentile(&sorted, 95),
            max_ms: sorted[sorted.len() - 1],
            mean_ms: sum / sorted.len() as u64,
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
    n: usize,
    syncs: usize,
    cache_type: CacheMode,
    launch_ms: u64,
    stats: Stats,
    samples_ms: &[u64],
) -> String {
    let samples = samples_ms
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"bench\": \"drive_sync_latency\",\n",
            "  \"started_at_unix\": {started_at_unix},\n",
            "  \"n\": {n},\n",
            "  \"syncs_per_sample\": {syncs},\n",
            "  \"cache_type\": \"{cache_type}\",\n",
            "  \"launch_ms\": {launch_ms},\n",
            "  \"unit\": \"ms\",\n",
            "  \"p50_ms\": {p50},\n",
            "  \"p95_ms\": {p95},\n",
            "  \"max_ms\": {max},\n",
            "  \"mean_ms\": {mean},\n",
            "  \"samples_ms\": [{samples}]\n",
            "}}"
        ),
        started_at_unix = started_at_unix,
        n = n,
        syncs = syncs,
        cache_type = cache_type.label(),
        launch_ms = launch_ms,
        p50 = stats.p50_ms,
        p95 = stats.p95_ms,
        max = stats.max_ms,
        mean = stats.mean_ms,
        samples = samples
    )
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
