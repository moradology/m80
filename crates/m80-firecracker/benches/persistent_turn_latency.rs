//! Real-KVM persistent-VM turn-to-turn latency probe.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::ExecRequest;

fn main() {
    let n = env_usize("N", 30);
    let output = std::env::var_os("M80_PERSISTENT_BENCH_OUTPUT").map(PathBuf::from);
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

    let vm_id = format!("persist-bench-{}", std::process::id());
    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");

    let warmup = running.exec(exec_request("warmup")).expect("warmup exec");
    assert_eq!(warmup.exit_code, Some(0), "warmup exec failed");

    let mut samples_ms = Vec::with_capacity(n);
    for i in 0..n {
        let t = Instant::now();
        let resp = running
            .exec(exec_request(&format!("turn-{i}")))
            .expect("persistent exec");
        let elapsed = t.elapsed();
        assert_eq!(resp.exit_code, Some(0), "persistent exec {i} failed");
        samples_ms.push(elapsed.as_micros() as u64);
    }

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");

    let stats = Stats::from_samples(&samples_ms);
    let json = render_json(started_at, n, stats, &samples_ms);
    println!("{json}");
    eprintln!(
        "persistent-turn latency: n={n} p50={}us p95={}us max={}us mean={}us",
        stats.p50_us, stats.p95_us, stats.max_us, stats.mean_us
    );
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create output parent");
        }
        fs::write(path, format!("{json}\n")).expect("write output");
    }
}

fn sandbox_config(vm_id: String) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id),
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

fn exec_request(label: &str) -> ExecRequest {
    ExecRequest {
        program: "/bin/echo".into(),
        args: vec![label.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
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

fn render_json(started_at_unix: u64, n: usize, stats: Stats, samples_us: &[u64]) -> String {
    let samples = samples_us
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"bench\": \"persistent_turn_latency\",\n",
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
        started_at_unix = started_at_unix,
        n = n,
        p50 = stats.p50_us,
        p95 = stats.p95_us,
        max = stats.max_us,
        mean = stats.mean_us,
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
