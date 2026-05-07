//! Real-KVM warm-pool allocation latency probe.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig, SnapshotPaths, WarmPool,
    WarmPoolConfig, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};

fn main() {
    let n = env_usize("N", 50);
    let target_ready = env_usize("M80_WARM_POOL_READY", 1);
    let load = std::env::var("M80_WARM_POOL_BENCH_LOAD").unwrap_or_else(|_| "idle".into());
    let output = std::env::var_os("M80_WARM_POOL_BENCH_OUTPUT").map(PathBuf::from);
    let started_at = unix_timestamp();
    let discovery = m80_preflight::run().expect("preflight");
    let snapshot_dir = discovery
        .run_root
        .join(format!("warm-pool-bench-snapshot-{}", std::process::id()));
    let paths = snapshot_paths(&snapshot_dir);

    let golden_backend = make_backend(discovery.clone(), 1);
    let golden = golden_backend
        .admit(sandbox_config(format!("wp-golden-{}", std::process::id())))
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

    let pool_backend = make_backend(discovery.clone(), (target_ready as u32) + 2);
    let pool = WarmPool::new(
        Arc::clone(&pool_backend),
        WarmPoolConfig {
            target_ready,
            snapshot: paths.clone(),
            sandbox: sandbox_config("warm-slot-template"),
            ready_probe: true_request(),
            vm_id_prefix: format!("wp-slot-{}", std::process::id()),
        },
    )
    .expect("WarmPool::new");
    pool.fill_to_target_blocking().expect("warm prefill");

    let mut allocation_samples_us = Vec::with_capacity(n);
    let mut refill_wait_samples_us = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        let mut lease = pool.try_lease().expect("warm lease");
        let resp = match lease.exec(m80_proto::ExecRequest {
            program: "/bin/true".into(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        }) {
            Ok(resp) => resp,
            Err(err) => {
                write_failure_artifacts(output.as_deref(), lease.vm_id(), lease.run_dir(), &err);
                panic!("warm exec: {err:?}");
            }
        };
        assert_eq!(resp.exit_code, Some(0), "warm exec must succeed");
        allocation_samples_us.push(t.elapsed().as_micros() as u64);

        lease.discard().expect("discard warm lease");
        let refill_t = Instant::now();
        pool.wait_for_ready(1, Duration::from_secs(10))
            .expect("pool refill");
        refill_wait_samples_us.push(refill_t.elapsed().as_micros() as u64);
    }

    if let Some(guard) = stress.as_mut() {
        guard.stop();
    }
    let _ = fs::remove_dir_all(&snapshot_dir);

    let allocation = Stats::from_samples(&allocation_samples_us);
    let refill = Stats::from_samples(&refill_wait_samples_us);
    let report = BenchReport {
        started_at_unix: started_at,
        load: &load,
        n,
        target_ready,
        allocation,
        refill,
        allocation_samples_us: &allocation_samples_us,
        refill_samples_us: &refill_wait_samples_us,
    };
    let json = render_json(report);
    println!("{json}");
    eprintln!(
        "warm-pool allocation: load={load} n={n} p50={}us p95={}us max={}us mean={}us refill_p95={}us",
        allocation.p50_us, allocation.p95_us, allocation.max_us, allocation.mean_us, refill.p95_us
    );
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create output parent");
        }
        fs::write(path, format!("{json}\n")).expect("write output");
    }
}

fn true_request() -> m80_proto::ExecRequest {
    m80_proto::ExecRequest {
        program: "/bin/true".into(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn write_failure_artifacts(
    output: Option<&Path>,
    vm_id: &str,
    run_dir: &Path,
    err: &dyn std::fmt::Debug,
) {
    let Some(output) = output else {
        eprintln!(
            "warm-pool failure: vm_id={vm_id} run_dir={}",
            run_dir.display()
        );
        return;
    };
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let failure_path = parent.join("warm-pool-allocation-failure.txt");
    let console_path = run_dir.join("console.log");
    let console = fs::read_to_string(&console_path)
        .unwrap_or_else(|e| format!("console.log unavailable at {}: {e}", console_path.display()));
    let body = format!(
        "vm_id={vm_id}\nrun_dir={}\nerror={err:?}\n\n--- console.log ---\n{console}\n",
        run_dir.display()
    );
    fs::write(&failure_path, body).expect("write warm-pool failure artifact");
    eprintln!("warm-pool failure artifact: {}", failure_path.display());
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

fn sandbox_config(vm_id: impl Into<String>) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(FIRST_LINE_MEM_SIZE_MIB),
        boot_args: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        idle_timeout: None,
        daemonize: false,
        request_id: None,
        preallocated_drive_slots: 0,
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

struct BenchReport<'a> {
    started_at_unix: u64,
    load: &'a str,
    n: usize,
    target_ready: usize,
    allocation: Stats,
    refill: Stats,
    allocation_samples_us: &'a [u64],
    refill_samples_us: &'a [u64],
}

fn render_json(report: BenchReport<'_>) -> String {
    let allocation_samples = report
        .allocation_samples_us
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let refill_samples = report
        .refill_samples_us
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"bench\": \"warm_pool_allocation_latency\",\n",
            "  \"load\": \"{load}\",\n",
            "  \"started_at_unix\": {started_at_unix},\n",
            "  \"n\": {n},\n",
            "  \"target_ready\": {target_ready},\n",
            "  \"unit\": \"us\",\n",
            "  \"allocation_p50_us\": {allocation_p50},\n",
            "  \"allocation_p95_us\": {allocation_p95},\n",
            "  \"allocation_max_us\": {allocation_max},\n",
            "  \"allocation_mean_us\": {allocation_mean},\n",
            "  \"refill_p50_us\": {refill_p50},\n",
            "  \"refill_p95_us\": {refill_p95},\n",
            "  \"refill_max_us\": {refill_max},\n",
            "  \"refill_mean_us\": {refill_mean},\n",
            "  \"allocation_samples_us\": [{allocation_samples}],\n",
            "  \"refill_samples_us\": [{refill_samples}]\n",
            "}}"
        ),
        load = report.load,
        started_at_unix = report.started_at_unix,
        n = report.n,
        target_ready = report.target_ready,
        allocation_p50 = report.allocation.p50_us,
        allocation_p95 = report.allocation.p95_us,
        allocation_max = report.allocation.max_us,
        allocation_mean = report.allocation.mean_us,
        refill_p50 = report.refill.p50_us,
        refill_p95 = report.refill.p95_us,
        refill_max = report.refill.max_us,
        refill_mean = report.refill.mean_us,
        allocation_samples = allocation_samples,
        refill_samples = refill_samples
    )
}

struct StressGuard {
    child: Option<Child>,
}

impl StressGuard {
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for StressGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start_stress() -> StressGuard {
    let procs = std::env::var("STRESS_PROCS").unwrap_or_else(|_| {
        std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "1".into())
    });
    let child = Command::new("stress-ng")
        .arg("--cpu")
        .arg(procs)
        .arg("--quiet")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start stress-ng");
    StressGuard { child: Some(child) }
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
