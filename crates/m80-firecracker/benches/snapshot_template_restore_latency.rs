//! Real-KVM snapshot-template restore-to-handback latency probe.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    Backend, BackendConfig, CgroupMode, HookSpec, HookSpecSet, HostnameSpec, NetworkPolicy,
    SandboxConfig, TemplateStore, WarmPool, WarmPoolConfig, WarmStrategy, FIRST_LINE_MEM_SIZE_MIB,
    FIRST_LINE_VCPU_COUNT,
};
use m80_image_manifest::KernelKind;
use m80_proto::ExecRequest;
use sha2::{Digest as _, Sha256};

const DEFAULT_N: usize = 20;
const DEFAULT_RUNS: usize = 3;
const DEFAULT_OUTPUT: &str =
    "crates/m80-firecracker/benches/snapshot_template_restore_latency.json";
const RESTORE_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    let preexisting_firecrackers = preexisting_firecracker_processes();
    let allow_other_firecracker_vms = env_bool("M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS");
    if !allow_other_firecracker_vms && !preexisting_firecrackers.is_empty() {
        panic!(
            "snapshot-template restore benchmark requires a quiet host; \
             found existing Firecracker processes: {preexisting_firecrackers:?}. \
             Stop them, or set M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=1 for a \
             non-closeable diagnostic run"
        );
    }

    let n = env_nonzero_usize("N", DEFAULT_N);
    let runs = env_nonzero_usize("M80_SNAPSHOT_TEMPLATE_RUNS", DEFAULT_RUNS);
    let load = std::env::var("M80_SNAPSHOT_BENCH_LOAD").unwrap_or_else(|_| "idle".into());
    let output = output_path_from_env();
    let git_worktree_dirty_excluding_artifact = git_worktree_dirty_excluding(&output);
    let git_commit = git_head_commit();
    let vcpu_count = env_nonzero_u32("M80_SNAPSHOT_BENCH_VCPU_COUNT", FIRST_LINE_VCPU_COUNT);
    let mem_size_mib = env_nonzero_u32("M80_SNAPSHOT_BENCH_MEM_SIZE_MIB", FIRST_LINE_MEM_SIZE_MIB);
    let jail_uid = env_u32("M80_JAIL_UID", 3000);
    let jail_gid = env_u32("M80_JAIL_GID", 3000);
    let started_at = unix_timestamp();
    let discovery = m80_preflight::run().expect("preflight");
    let store_root = discovery.run_root.join("warm").join(format!(
        "snapshot-template-restore-bench-{}",
        std::process::id()
    ));
    fs::create_dir_all(&store_root).expect("create snapshot-template bench store parent");
    let store = Arc::new(TemplateStore::create(store_root.join("templates"), 8).expect("store"));

    drop_page_cache();
    let mut stress = if load == "loaded" {
        Some(start_stress())
    } else {
        None
    };

    let mut run_results = Vec::with_capacity(runs);
    for run_index in 0..runs {
        run_results.push(run_one(
            discovery.clone(),
            Arc::clone(&store),
            run_index,
            n,
            vcpu_count,
            mem_size_mib,
            jail_uid,
            jail_gid,
        ));
    }

    if let Some(child) = stress.as_mut() {
        stop_stress(child);
    }
    let post_run_firecrackers = preexisting_firecracker_processes();

    let report = BenchReport {
        started_at_unix: started_at,
        load: &load,
        n,
        runs,
        output: &output,
        target_ready: 1,
        vcpu_count,
        mem_size_mib,
        jail_uid,
        jail_gid,
        store_root: &store_root,
        discovery: &discovery,
        allow_other_firecracker_vms,
        preexisting_firecrackers: &preexisting_firecrackers,
        post_run_firecrackers: &post_run_firecrackers,
        git_worktree_dirty_excluding_artifact,
        git_commit: &git_commit,
        run_results: &run_results,
    };
    let json = render_json(&report);
    println!("{json}");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create output parent");
    }
    fs::write(&output, format!("{json}\n")).expect("write snapshot-template restore bench output");
    eprintln!(
        "snapshot-template restore: load={load} runs={runs} n={n} p99={}us output={}",
        all_stats(&run_results).p99_us,
        display_output_path(&output)
    );

    let _ = fs::remove_dir_all(&store_root);
}

fn run_one(
    discovery: m80_preflight::Discovery,
    store: Arc<TemplateStore>,
    run_index: usize,
    n: usize,
    vcpu_count: u32,
    mem_size_mib: u32,
    jail_uid: u32,
    jail_gid: u32,
) -> RunResult {
    let backend = make_backend(discovery, 4, jail_uid, jail_gid);
    let hooks = measurement_hooks();
    let pool = WarmPool::new(
        Arc::clone(&backend),
        WarmPoolConfig {
            target_ready: 1,
            sandbox: sandbox_config(
                format!("tpl-bench-run-{run_index}-slot"),
                vcpu_count,
                mem_size_mib,
            ),
            strategy: WarmStrategy::snapshot_restore(store, hooks, true_request()),
            vm_id_prefix: format!("tplb-{run_index}-{}", std::process::id()),
            cpu_allocator: None,
        },
    )
    .expect("WarmPool::new");

    pool.fill_to_target_blocking()
        .expect("initial template-backed fill");
    let initial_fill_samples_us = pool.take_fill_duration_samples_us();

    drop_page_cache();
    let mut current = pool.try_lease().expect("initial warm lease");
    let mut samples = Vec::with_capacity(n);
    for cycle in 0..n {
        pool.wait_for_ready(1, RESTORE_TIMEOUT)
            .expect("replacement template restore");
        let fill_us = take_one_fill_sample(&pool, run_index, cycle);
        drop_page_cache();
        let handoff_started = Instant::now();
        let next = pool.try_lease().expect("replacement warm lease");
        let handoff_us = duration_micros_u64(handoff_started.elapsed());
        let phase_durations_us = phase_durations(next.run_dir());
        samples.push(Sample {
            run: run_index,
            cycle,
            fill_us,
            handoff_us,
            restore_to_handback_us: fill_us.saturating_add(handoff_us),
            phase_durations_us,
        });
        current.discard().expect("discard previous lease");
        current = next;
    }
    current.discard().expect("discard final lease");

    RunResult {
        run: run_index,
        initial_fill_samples_us,
        samples,
    }
}

fn take_one_fill_sample(pool: &WarmPool, run: usize, cycle: usize) -> u64 {
    let samples = pool.take_fill_duration_samples_us();
    assert_eq!(
        samples.len(),
        1,
        "expected exactly one fill-duration sample for run={run} cycle={cycle}, got {samples:?}"
    );
    samples[0]
}

fn measurement_hooks() -> HookSpecSet {
    HookSpecSet::new(vec![
        HookSpec::ReseedSystemdRandomSeed,
        HookSpec::RegenMachineId,
        HookSpec::SetHostname(HostnameSpec::new("m80-template-bench").expect("hostname")),
    ])
}

fn true_request() -> ExecRequest {
    ExecRequest {
        program: "/bin/true".to_owned(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn make_backend(
    discovery: m80_preflight::Discovery,
    max_concurrent_vms: u32,
    jail_uid: u32,
    jail_gid: u32,
) -> Arc<Backend> {
    let run_root = discovery.run_root.clone();
    Arc::new(
        Backend::new(
            BackendConfig::builder(discovery)
                .max_concurrent_vms(max_concurrent_vms)
                .run_root(run_root)
                .jail_uid(jail_uid)
                .jail_gid(jail_gid)
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
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        drive_cache_type: None,
        boot_args: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        overlay_clone_mode: Default::default(),
        idle_timeout: None,
        daemonize: false,
        request_id: None,
        preallocated_drive_slots: 0,
        pmem_layers: Vec::new(),
        one_shot: false,
    }
}

fn drop_page_cache() {
    let status = Command::new("sh")
        .arg("-c")
        .arg("sync; echo 3 > /proc/sys/vm/drop_caches")
        .status()
        .expect("drop page cache command");
    assert!(status.success(), "drop page cache command failed: {status}");
}

#[derive(Debug)]
struct Sample {
    run: usize,
    cycle: usize,
    fill_us: u64,
    handoff_us: u64,
    restore_to_handback_us: u64,
    phase_durations_us: BTreeMap<String, u64>,
}

struct RunResult {
    run: usize,
    initial_fill_samples_us: Vec<u64>,
    samples: Vec<Sample>,
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
    let rank = ((sorted.len() as f64) * (pct as f64 / 100.0)).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn git_worktree_dirty_excluding(path: &Path) -> bool {
    let repo = repo_root();
    let rel = artifact_relative_path(path, &repo);
    let output = Command::new("git")
        .args([
            "-C",
            &repo.display().to_string(),
            "status",
            "--porcelain=v1",
        ])
        .output();
    let Ok(output) = output else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(|line| {
            let path = line
                .strip_prefix("?? ")
                .or_else(|| line.get(3..))
                .unwrap_or(line)
                .trim();
            path != rel
        })
}

fn artifact_relative_path(path: &Path, repo: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.join(path)
    };
    absolute
        .strip_prefix(repo)
        .ok()
        .and_then(|path| path.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn git_head_commit() -> String {
    let repo = repo_root();
    let output = Command::new("git")
        .args(["-C", &repo.display().to_string(), "rev-parse", "HEAD"])
        .output()
        .expect("spawn git rev-parse HEAD");
    assert!(output.status.success(), "git rev-parse HEAD failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct BenchReport<'a> {
    started_at_unix: u64,
    load: &'a str,
    n: usize,
    runs: usize,
    output: &'a Path,
    target_ready: usize,
    vcpu_count: u32,
    mem_size_mib: u32,
    jail_uid: u32,
    jail_gid: u32,
    store_root: &'a Path,
    discovery: &'a m80_preflight::Discovery,
    allow_other_firecracker_vms: bool,
    preexisting_firecrackers: &'a [FirecrackerProcess],
    post_run_firecrackers: &'a [FirecrackerProcess],
    git_worktree_dirty_excluding_artifact: bool,
    git_commit: &'a str,
    run_results: &'a [RunResult],
}

fn render_json(report: &BenchReport<'_>) -> String {
    let all = report
        .run_results
        .iter()
        .flat_map(|run| run.samples.iter())
        .collect::<Vec<_>>();
    let restore_to_handback = samples_for(&all, |sample| sample.restore_to_handback_us);
    let fill = samples_for(&all, |sample| sample.fill_us);
    let handoff = samples_for(&all, |sample| sample.handoff_us);
    let payload = serde_json::json!({
        "schema_version": 1,
        "bench": "snapshot_template_restore_latency",
        "started_at_unix": report.started_at_unix,
        "load": report.load,
        "n_per_run": report.n,
        "runs": report.runs,
        "samples_total": restore_to_handback.len(),
        "target_ready": report.target_ready,
        "vcpu_count": report.vcpu_count,
        "mem_size_mib": report.mem_size_mib,
        "jail_uid": report.jail_uid,
        "jail_gid": report.jail_gid,
        "cgroup_mode": "disabled",
        "page_cache_dropped_between_samples": true,
        "git_worktree_dirty_excluding_artifact": report.git_worktree_dirty_excluding_artifact,
        "git_commit": report.git_commit,
        "reproduction_command": reproduction_command(report),
        "store_root": report.store_root,
        "substrate": {
            "substrate_kind": "real-kvm",
            "preflight_required": true,
            "quiet_host_checked": true,
            "allow_other_firecracker_vms": report.allow_other_firecracker_vms,
            "preexisting_firecracker_processes": report
                .preexisting_firecrackers
                .iter()
                .map(firecracker_process_json)
                .collect::<Vec<_>>(),
            "post_run_firecracker_processes": report
                .post_run_firecrackers
                .iter()
                .map(firecracker_process_json)
                .collect::<Vec<_>>(),
            "host_kernel_release": command_output("uname", &["-r"]),
            "dev_kvm_stat": command_output("stat", &["-c", "%A %U:%G %n", "/dev/kvm"]),
            "sudo_uid": command_output("id", &["-u"]),
            "firecracker_version": command_output(report.discovery.firecracker_bin.as_os_str(), &["--version"]),
            "preflight_artifacts": preflight_artifacts_json(report.discovery),
        },
        "data": {
            "warm": {
                "restore_to_handback_ms": stats_json_ms(Stats::from_samples(&restore_to_handback)),
                "restore_to_handback_us": stats_json_us(Stats::from_samples(&restore_to_handback)),
                "fill_us": stats_json_us(Stats::from_samples(&fill)),
                "handoff_us": stats_json_us(Stats::from_samples(&handoff)),
                "samples_us": restore_to_handback,
                "sample_details": all
                    .iter()
                    .map(|sample| serde_json::json!({
                        "run": sample.run,
                        "cycle": sample.cycle,
                        "fill_us": sample.fill_us,
                        "handoff_us": sample.handoff_us,
                        "restore_to_handback_us": sample.restore_to_handback_us,
                        "phase_durations_us": &sample.phase_durations_us,
                    }))
                    .collect::<Vec<_>>(),
            },
        },
        "runs_detail": report
            .run_results
            .iter()
            .map(run_json)
            .collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&payload).expect("render snapshot-template restore bench json")
}

fn reproduction_command(report: &BenchReport<'_>) -> String {
    let mut parts = vec![
        env_assignment(
            "M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS",
            if report.allow_other_firecracker_vms {
                "1"
            } else {
                "0"
            },
        ),
        env_assignment("M80_SNAPSHOT_BENCH_LOAD", report.load),
        env_assignment("N", &report.n.to_string()),
        env_assignment("M80_SNAPSHOT_TEMPLATE_RUNS", &report.runs.to_string()),
        env_assignment(
            "M80_SNAPSHOT_BENCH_VCPU_COUNT",
            &report.vcpu_count.to_string(),
        ),
        env_assignment(
            "M80_SNAPSHOT_BENCH_MEM_SIZE_MIB",
            &report.mem_size_mib.to_string(),
        ),
        env_assignment("M80_JAIL_UID", &report.jail_uid.to_string()),
        env_assignment("M80_JAIL_GID", &report.jail_gid.to_string()),
        env_assignment("M80_CGROUP_MODE", "disabled"),
        env_assignment(
            "M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT",
            &display_output_path(report.output),
        ),
        env_assignment(
            "M80_FIRECRACKER_BIN",
            &report.discovery.firecracker_bin.display().to_string(),
        ),
        env_assignment(
            "M80_JAILER_BIN",
            &report.discovery.jailer_bin.display().to_string(),
        ),
        env_assignment(
            "M80_FIRECRACKER_SECCOMP_FILTER",
            &report
                .discovery
                .firecracker_seccomp_filter
                .display()
                .to_string(),
        ),
        env_assignment(
            "M80_JAILER_HARDEN_BIN",
            &report.discovery.jailer_harden_bin.display().to_string(),
        ),
        env_assignment(
            "M80_NET_HELPER_BIN",
            &report.discovery.net_helper_bin.display().to_string(),
        ),
        env_assignment(
            "M80_KERNEL_IMAGE",
            &report.discovery.kernel.display().to_string(),
        ),
        env_assignment(
            "M80_KERNEL_KIND",
            kernel_kind_env(report.discovery.manifest.kernel_kind),
        ),
        env_assignment(
            "M80_ROOTFS_IMAGE",
            &report.discovery.rootfs.display().to_string(),
        ),
        env_assignment(
            "M80_RUN_ROOT",
            &report.discovery.run_root.display().to_string(),
        ),
    ];
    parts.push(
        "cargo bench -p m80-firecracker --bench snapshot_template_restore_latency".to_owned(),
    );
    parts.join(" ")
}

fn kernel_kind_env(kind: KernelKind) -> &'static str {
    match kind {
        KernelKind::Stock => "stock",
        KernelKind::Stripped => "stripped",
    }
}

fn env_assignment(name: &str, value: &str) -> String {
    format!("{name}={}", shell_escape(value))
}

fn output_path_from_env() -> PathBuf {
    let raw = std::env::var_os("M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT));
    if raw.is_absolute() {
        raw
    } else {
        repo_root().join(raw)
    }
}

fn display_output_path(path: &Path) -> String {
    let repo = repo_root();
    path.strip_prefix(&repo)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn shell_escape(value: &str) -> String {
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b':' | b'='))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn preflight_artifacts_json(discovery: &m80_preflight::Discovery) -> serde_json::Value {
    serde_json::json!({
        "firecracker_bin": discovery.firecracker_bin,
        "firecracker_seccomp_filter": discovery.firecracker_seccomp_filter,
        "jailer_bin": discovery.jailer_bin,
        "jailer_harden_bin": discovery.jailer_harden_bin,
        "net_helper_bin": discovery.net_helper_bin,
        "kernel_image": discovery.kernel,
        "rootfs_image": discovery.rootfs,
        "kernel_image_sha256": sha256_file_hex(&discovery.kernel),
        "rootfs_image_sha256": sha256_file_hex(&discovery.rootfs),
        "kernel_kind": discovery.manifest.kernel_kind,
        "image_kind": discovery.manifest.image_kind,
        "rootfs_format": discovery.manifest.rootfs_format,
        "expected_firecracker_version": discovery.manifest.expected_firecracker_version,
    })
}

fn sha256_file_hex(path: &Path) -> String {
    let mut file = fs::File::open(path)
        .unwrap_or_else(|err| panic!("open {} for sha256: {err}", path.display()));
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .unwrap_or_else(|err| panic!("hash {} for sha256: {err}", path.display()));
    hex::encode(hasher.finalize())
}

fn command_output(command: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> String {
    match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        Ok(output) => format!("exit status {}", output.status),
        Err(err) => format!("error: {err}"),
    }
}

#[derive(Debug)]
struct FirecrackerProcess {
    pid: u32,
    argv: Vec<String>,
}

fn preexisting_firecracker_processes() -> Vec<FirecrackerProcess> {
    let proc = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(err) => panic!("read /proc: {err}"),
    };
    let mut processes = Vec::new();
    for entry in proc {
        let entry = entry.unwrap_or_else(|err| panic!("read /proc entry: {err}"));
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(|raw| raw.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let argv = cmdline
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>();
        let Some(program) = argv.first() else {
            continue;
        };
        let basename = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program);
        if basename == "firecracker" {
            processes.push(FirecrackerProcess { pid, argv });
        }
    }
    processes.sort_by_key(|process| process.pid);
    processes
}

fn firecracker_process_json(process: &FirecrackerProcess) -> serde_json::Value {
    serde_json::json!({
        "pid": process.pid,
        "argv": process.argv,
    })
}

fn samples_for(samples: &[&Sample], field: impl Fn(&Sample) -> u64) -> Vec<u64> {
    samples.iter().map(|sample| field(sample)).collect()
}

fn run_json(run: &RunResult) -> serde_json::Value {
    let restore = run
        .samples
        .iter()
        .map(|sample| sample.restore_to_handback_us)
        .collect::<Vec<_>>();
    serde_json::json!({
        "run": run.run,
        "initial_fill_samples_us": &run.initial_fill_samples_us,
        "restore_to_handback_us": stats_json_us(Stats::from_samples(&restore)),
        "samples_us": restore,
    })
}

fn stats_json_us(stats: Stats) -> serde_json::Value {
    serde_json::json!({
        "p50": stats.p50_us,
        "p95": stats.p95_us,
        "p99": stats.p99_us,
        "max": stats.max_us,
        "mean": stats.mean_us,
    })
}

fn stats_json_ms(stats: Stats) -> serde_json::Value {
    serde_json::json!({
        "p50": us_to_ms(stats.p50_us),
        "p95": us_to_ms(stats.p95_us),
        "p99": us_to_ms(stats.p99_us),
        "max": us_to_ms(stats.max_us),
        "mean": us_to_ms(stats.mean_us),
    })
}

fn us_to_ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

fn all_stats(results: &[RunResult]) -> Stats {
    let samples = results
        .iter()
        .flat_map(|run| {
            run.samples
                .iter()
                .map(|sample| sample.restore_to_handback_us)
        })
        .collect::<Vec<_>>();
    Stats::from_samples(&samples)
}

fn phase_durations(run_dir: &Path) -> BTreeMap<String, u64> {
    let path = run_dir.join(m80_observability::DIAGNOSTICS_FILE_NAME);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read diagnostics at {}: {err}", path.display()));
    let mut out = BTreeMap::new();
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
        out.insert(name.to_owned(), duration_us);
    }
    out
}

fn duration_micros_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
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
    let value = env_u32(name, default);
    assert!(value > 0, "{name} must be greater than zero");
    value
}

fn env_bool(name: &str) -> bool {
    match std::env::var(name).as_deref() {
        Ok("1" | "true" | "TRUE" | "yes" | "YES") => true,
        Ok("0" | "false" | "FALSE" | "no" | "NO") | Err(_) => false,
        Ok(raw) => panic!("{name} must be boolean-like, got {raw:?}"),
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
