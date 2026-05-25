//! Real-KVM snapshot-restore ready-latency probe.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig, SnapshotPaths,
    FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};

const DEFAULT_FILE_READ_SAMPLES: usize = 10;

fn main() {
    let n = env_nonzero_usize("N", 50);
    let file_read_samples =
        env_nonzero_usize("M80_RESTORE_FILE_READ_SAMPLES", DEFAULT_FILE_READ_SAMPLES);
    let vcpu_count = env_nonzero_u32("M80_SNAPSHOT_BENCH_VCPU_COUNT", FIRST_LINE_VCPU_COUNT);
    let mem_size_mib = env_nonzero_u32("M80_SNAPSHOT_BENCH_MEM_SIZE_MIB", FIRST_LINE_MEM_SIZE_MIB);
    let load = std::env::var("M80_SNAPSHOT_BENCH_LOAD").unwrap_or_else(|_| "idle".into());
    let output = std::env::var_os("M80_SNAPSHOT_BENCH_OUTPUT").map(PathBuf::from);
    let started_at = unix_timestamp();
    let discovery = m80_preflight::run().expect("preflight");
    let snapshot_dir = discovery
        .run_root
        .join("warm")
        .join(format!("m80-snapshot-restore-bench-{}", std::process::id()));
    let paths = snapshot_paths(&snapshot_dir);

    let golden_backend = make_backend(discovery.clone(), 1);
    let golden = golden_backend
        .admit(sandbox_config(
            format!("sbg-{}", std::process::id()),
            vcpu_count,
            mem_size_mib,
        ))
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
    let warm = run_restore_mode(
        &restore_backend,
        &discovery,
        &paths,
        n,
        CacheMode::Warm,
        vcpu_count,
        mem_size_mib,
    );
    let cold = run_restore_mode(
        &restore_backend,
        &discovery,
        &paths,
        n,
        CacheMode::Cold,
        vcpu_count,
        mem_size_mib,
    );

    if let Some(child) = stress.as_mut() {
        stop_stress(child);
    }

    let file_reads = measure_snapshot_file_reads(&paths, file_read_samples);
    let report = BenchReport {
        started_at_unix: started_at,
        load: &load,
        n,
        file_read_samples,
        vcpu_count,
        mem_size_mib,
        snapshot_paths: &paths,
        warm,
        cold,
        file_reads,
    };
    let json = render_json(&report);
    println!("{json}");
    eprintln!(
        "snapshot-restore baseline: load={load} n={n} warm_p50={}us cold_p50={}us cold_p95={}us",
        report.warm.stats.p50_us, report.cold.stats.p50_us, report.cold.stats.p95_us
    );
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create output parent");
        }
        fs::write(path, format!("{json}\n")).expect("write output");
    }

    let _ = fs::remove_dir_all(&snapshot_dir);
}

fn run_restore_mode(
    backend: &Arc<Backend>,
    discovery: &m80_preflight::Discovery,
    paths: &SnapshotPaths,
    n: usize,
    mode: CacheMode,
    vcpu_count: u32,
    mem_size_mib: u32,
) -> ModeResult {
    let mut samples_us = Vec::with_capacity(n);
    let mut phases = PhaseSamples::default();
    for i in 0..n {
        if mode == CacheMode::Cold {
            drop_page_cache();
        }
        let sandbox = backend
            .admit(sandbox_config(
                format!("sbr-{}-{}-{i}", mode.name(), std::process::id()),
                vcpu_count,
                mem_size_mib,
            ))
            .expect("admit restore");
        let t = Instant::now();
        let restored = sandbox
            .launch_from_snapshot(paths.clone(), discovery)
            .expect("launch_from_snapshot");
        samples_us.push(t.elapsed().as_micros() as u64);
        phases.record_run_dir(restored.run_dir());
        restored
            .stop()
            .expect("stop restored")
            .delete()
            .expect("delete restored");
    }

    ModeResult {
        cache_mode: mode,
        stats: Stats::from_samples(&samples_us),
        samples_us,
        phases,
    }
}

fn measure_snapshot_file_reads(paths: &SnapshotPaths, samples: usize) -> Vec<FileReadResult> {
    [
        ("vm.snap", paths.vm_state.as_path()),
        ("mem.snap", paths.mem.as_path()),
    ]
    .into_iter()
    .map(|(name, path)| measure_one_snapshot_file(name, path, samples))
    .collect()
}

fn measure_one_snapshot_file(name: &str, path: &Path, samples: usize) -> FileReadResult {
    let bytes = fs::metadata(path).expect("stat snapshot file").len();
    let mut warm_samples = Vec::with_capacity(samples);
    let mut cold_samples = Vec::with_capacity(samples);

    for _ in 0..samples {
        read_file_to_sink(path).expect("prime warm snapshot file read");
        warm_samples.push(timed_file_read_us(path));
    }
    for _ in 0..samples {
        drop_page_cache();
        cold_samples.push(timed_file_read_us(path));
    }

    FileReadResult {
        name: name.to_owned(),
        bytes,
        warm: Stats::from_samples(&warm_samples),
        cold: Stats::from_samples(&cold_samples),
        warm_samples_us: warm_samples,
        cold_samples_us: cold_samples,
    }
}

fn timed_file_read_us(path: &Path) -> u64 {
    let started = Instant::now();
    read_file_to_sink(path).expect("read snapshot file");
    started.elapsed().as_micros() as u64
}

fn read_file_to_sink(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    io::copy(&mut file, &mut io::sink())
}

fn drop_page_cache() {
    let status = Command::new("sh")
        .arg("-c")
        .arg("sync; echo 3 > /proc/sys/vm/drop_caches")
        .status()
        .expect("drop page cache command");
    assert!(status.success(), "drop page cache command failed: {status}");
}

fn make_backend(discovery: m80_preflight::Discovery, max_concurrent_vms: u32) -> Arc<Backend> {
    let run_root = discovery.run_root.clone();
    Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(max_concurrent_vms)
                .run_root(run_root)
                .jail_uid(env_u32("M80_JAIL_UID", 3000))
                .jail_gid(env_u32("M80_JAIL_GID", 3000))
                .cgroup_mode(CgroupMode::Disabled)
                .build(),
        )
        .expect("Backend::new"),
    )
}

fn sandbox_config(vm_id: String, vcpu_count: u32, mem_size_mib: u32) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id),
        workspace: None,
        network: NetworkPolicy::NoEgress,
        vcpu_count: Some(vcpu_count),
        mem_size_mib: Some(mem_size_mib),
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

fn snapshot_paths(dir: &Path) -> SnapshotPaths {
    fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CacheMode {
    Warm,
    Cold,
}

impl CacheMode {
    fn name(self) -> &'static str {
        match self {
            CacheMode::Warm => "warm",
            CacheMode::Cold => "cold",
        }
    }
}

struct BenchReport<'a> {
    started_at_unix: u64,
    load: &'a str,
    n: usize,
    file_read_samples: usize,
    vcpu_count: u32,
    mem_size_mib: u32,
    snapshot_paths: &'a SnapshotPaths,
    warm: ModeResult,
    cold: ModeResult,
    file_reads: Vec<FileReadResult>,
}

struct ModeResult {
    cache_mode: CacheMode,
    stats: Stats,
    samples_us: Vec<u64>,
    phases: PhaseSamples,
}

struct FileReadResult {
    name: String,
    bytes: u64,
    warm: Stats,
    cold: Stats,
    warm_samples_us: Vec<u64>,
    cold_samples_us: Vec<u64>,
}

#[derive(Clone, Copy)]
struct Stats {
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
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
            p99_us: percentile(&sorted, 99),
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

#[derive(Default)]
struct PhaseSamples {
    by_name: BTreeMap<String, Vec<u64>>,
}

impl PhaseSamples {
    fn record_run_dir(&mut self, run_dir: &Path) {
        let path = run_dir.join(m80_observability::DIAGNOSTICS_FILE_NAME);
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read diagnostics at {}: {e}", path.display()));
        for line in raw.lines() {
            let event: serde_json::Value =
                serde_json::from_str(line).expect("diagnostics line is json");
            if event["event_kind"] != "phase_completed" {
                continue;
            }
            let Some(name) = event["context"]["phase_name"].as_str() else {
                continue;
            };
            let Some(duration_us) = event["duration_us"].as_u64() else {
                continue;
            };
            self.by_name
                .entry(name.to_owned())
                .or_default()
                .push(duration_us);
        }
    }
}

fn render_json(report: &BenchReport<'_>) -> String {
    let restore_delta_p50 = report
        .cold
        .stats
        .p50_us
        .saturating_sub(report.warm.stats.p50_us);
    let snapshot_read_delta_p50 = report
        .file_reads
        .iter()
        .map(|file| file.cold.p50_us.saturating_sub(file.warm.p50_us))
        .sum::<u64>();
    let residual_delta_p50 = restore_delta_p50.saturating_sub(snapshot_read_delta_p50);
    let payload = serde_json::json!({
        "schema_version": 1,
        "bench": "snapshot_restore_cold_baseline",
        "load": report.load,
        "started_at_unix": report.started_at_unix,
        "n": report.n,
        "file_read_samples": report.file_read_samples,
        "vcpu_count": report.vcpu_count,
        "mem_size_mib": report.mem_size_mib,
        "unit": "us",
        "snapshot_files": {
            "vm_snap": {
                "path": &report.snapshot_paths.vm_state,
                "bytes": file_len(&report.snapshot_paths.vm_state),
            },
            "mem_snap": {
                "path": &report.snapshot_paths.mem,
                "bytes": file_len(&report.snapshot_paths.mem),
            },
        },
        "restore_modes": [mode_json(&report.warm), mode_json(&report.cold)],
        "snapshot_file_reads": report
            .file_reads
            .iter()
            .map(file_read_json)
            .collect::<Vec<_>>(),
        "cold_warm_delta_p50_us": {
            "restore_total": restore_delta_p50,
            "direct_snapshot_file_reads": snapshot_read_delta_p50,
            "residual_other": residual_delta_p50,
        },
    });
    serde_json::to_string_pretty(&payload).expect("render restore bench json")
}

fn mode_json(mode: &ModeResult) -> serde_json::Value {
    serde_json::json!({
        "cache_mode": mode.cache_mode.name(),
        "p50_us": mode.stats.p50_us,
        "p95_us": mode.stats.p95_us,
        "p99_us": mode.stats.p99_us,
        "max_us": mode.stats.max_us,
        "mean_us": mode.stats.mean_us,
        "samples_us": &mode.samples_us,
        "phases": phases_json(&mode.phases),
    })
}

fn phases_json(phases: &PhaseSamples) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (name, samples) in &phases.by_name {
        let stats = Stats::from_samples(samples);
        out.insert(
            name.clone(),
            serde_json::json!({
                "count": samples.len(),
                "p50_us": stats.p50_us,
                "p95_us": stats.p95_us,
                "p99_us": stats.p99_us,
                "max_us": stats.max_us,
                "mean_us": stats.mean_us,
            }),
        );
    }
    serde_json::Value::Object(out)
}

fn file_read_json(file: &FileReadResult) -> serde_json::Value {
    serde_json::json!({
        "file": &file.name,
        "bytes": file.bytes,
        "warm": stats_json(file.warm),
        "cold": stats_json(file.cold),
        "cold_warm_delta_p50_us": file.cold.p50_us.saturating_sub(file.warm.p50_us),
        "warm_samples_us": &file.warm_samples_us,
        "cold_samples_us": &file.cold_samples_us,
    })
}

fn stats_json(stats: Stats) -> serde_json::Value {
    serde_json::json!({
        "p50_us": stats.p50_us,
        "p95_us": stats.p95_us,
        "p99_us": stats.p99_us,
        "max_us": stats.max_us,
        "mean_us": stats.mean_us,
    })
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).expect("stat snapshot path").len()
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

fn env_nonzero_usize(name: &str, default: usize) -> usize {
    let value = std::env::var(name).map_or(default, |raw| {
        raw.parse()
            .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}"))
    });
    assert!(value > 0, "{name} must be greater than zero");
    value
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_nonzero_u32(name: &str, default: u32) -> u32 {
    let value = std::env::var(name).map_or(default, |raw| {
        raw.parse()
            .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}"))
    });
    assert!(value > 0, "{name} must be greater than zero");
    value
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
