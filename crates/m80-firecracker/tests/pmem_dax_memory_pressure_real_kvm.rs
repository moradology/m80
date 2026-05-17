//! Phase G memory-pressure measurement for Shared pmem DAX pages.
//!
//! This measurement-shaped scenario is owned by `m80-q420k.8.9`. The test is
//! ignored by default and writes `docs/perf/pmem-dax-memory-pressure.md` only
//! when explicitly enabled on a real-KVM host.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use m80_image_store::DEFAULT_STORE_ROOT;

mod common;
mod pmem_shared_support;
#[path = "e2e_composed_real_kvm/quiet_host.rs"]
mod quiet_host;

use pmem_shared_support as support;

const DEFAULT_VM_COUNT: usize = 2;
const DEFAULT_SAMPLES: usize = 5;
const DEFAULT_PAYLOAD_MIB: usize = 32;
const DEFAULT_SETTLE_MS: u64 = 1_000;

#[test]
#[ignore = "measurement-shaped; set M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1"]
fn pmem_dax_memory_pressure_real_kvm() {
    if std::env::var("M80_RUN_PMEM_DAX_MEMORY_PRESSURE").as_deref() != Ok("1") {
        eprintln!("SKIP: set M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 to write the measurement artifact");
        return;
    }

    let pressure_command = std::env::var("M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND").expect(
        "M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND must name the host or sibling-VM pressure workload",
    );

    let _serial = support::REAL_KVM_LOCK.lock().expect("real-kvm test lock");
    let (allow_other_firecracker_vms, preexisting_firecrackers) =
        quiet_host::assert_or_record_quiet_host_for(
            "pmem DAX memory pressure",
            "M80_PMEM_DAX_MEMORY_PRESSURE_ALLOW_OTHER_VMS",
        );
    let mut substrate =
        quiet_host::substrate_json(allow_other_firecracker_vms, &preexisting_firecrackers);

    let vm_count = env_usize("M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT", DEFAULT_VM_COUNT);
    let samples = env_usize("M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES", DEFAULT_SAMPLES);
    let payload_mib = env_usize(
        "M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB",
        DEFAULT_PAYLOAD_MIB,
    );
    let settle = Duration::from_millis(env_u64(
        "M80_PMEM_DAX_MEMORY_PRESSURE_SETTLE_MS",
        DEFAULT_SETTLE_MS,
    ));
    assert!(
        vm_count >= 2,
        "memory-pressure measurement requires >=2 VMs"
    );
    assert!(
        samples >= 3,
        "memory-pressure measurement requires >=3 samples"
    );

    let run_root = env_path("M80_RUN_ROOT").unwrap_or_else(|| PathBuf::from("/var/lib/m80"));
    let artifact = env_path("M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT")
        .unwrap_or_else(|| repo_root().join("docs/perf/pmem-dax-memory-pressure.md"));
    let git_worktree_dirty =
        quiet_host::git_worktree_dirty_excluding(std::slice::from_ref(&artifact));
    let git_commit = quiet_host::git_head_commit();

    let store = support::open_default_store();
    let digest = support::build_payload_image_digest(&store, payload_mib);
    let store_path = support::store_erofs_path(&store, &digest);
    let payload_layout =
        support::assert_payload_file_uncompressed_non_inlined(&store_path, "payload.bin");
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    quiet_host::record_preflight_artifacts(&mut substrate, &discovery);
    let real = support::real_backend_from_discovery(discovery, vm_count as u32);

    let process_count_before = process_count_by_basename(&["firecracker", "jailer"]);
    let mount_count_before = mount_count_under(&run_root);

    let mut running = Vec::with_capacity(vm_count);
    for idx in 0..vm_count {
        let mut sandbox = support::launch_with_layers(
            &real.backend,
            &format!("pdmp-{idx}"),
            vec![support::shared_layer(&digest, 0)],
        );
        support::assert_one_pmem_mount(&mut sandbox, 0);
        running.push(sandbox);
    }
    assert_eq!(
        store.shared_ref_count(&digest).unwrap(),
        vm_count,
        "all pressure VMs must hold Shared active-use markers"
    );

    let baseline = measure_all_guests(&mut running, samples);
    let mem_before = meminfo();
    let mut pressure = Command::new("sh")
        .arg("-c")
        .arg(&pressure_command)
        .spawn()
        .unwrap_or_else(|err| panic!("spawn pressure command {pressure_command:?}: {err}"));
    std::thread::sleep(settle);
    let mem_during = meminfo();
    let status = pressure
        .wait()
        .unwrap_or_else(|err| panic!("wait for pressure command {pressure_command:?}: {err}"));
    assert!(
        status.success(),
        "pressure command {pressure_command:?} exited {status}"
    );

    let post_pressure = measure_all_guests(&mut running, samples);
    let mem_after = meminfo();

    while let Some(sandbox) = running.pop() {
        support::stop_and_delete(sandbox);
    }
    let leaked_markers = store.shared_ref_count(&digest).unwrap();
    let stale_markers = store
        .sweep_shared_refs(std::iter::empty::<&str>())
        .expect("post-pressure stale marker sweep");
    let process_count_after = process_count_by_basename(&["firecracker", "jailer"]);
    let mount_count_after = mount_count_under(&run_root);
    quiet_host::record_post_run_firecracker_processes(
        &mut substrate,
        &quiet_host::firecracker_processes(),
    );

    let report = PressureReport {
        vm_count,
        samples,
        payload_mib,
        digest: digest.as_str().to_owned(),
        store_path,
        payload_layout,
        substrate,
        git_worktree_dirty,
        git_commit,
        pressure_command,
        baseline,
        post_pressure,
        mem_before,
        mem_during,
        mem_after,
        leaked_markers: leaked_markers + stale_markers,
        leaked_processes: process_count_after.saturating_sub(process_count_before),
        leaked_mounts: mount_count_after.saturating_sub(mount_count_before),
    };
    write_artifact(&artifact, &report);
    eprintln!(
        "M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT {}",
        artifact.display()
    );
}

fn measure_all_guests(
    running: &mut [m80_firecracker::RunningSandbox],
    samples: usize,
) -> Vec<LatencyStats> {
    running
        .iter_mut()
        .map(|sandbox| measure_guest_reads(sandbox, samples))
        .collect()
}

fn measure_guest_reads(
    running: &mut m80_firecracker::RunningSandbox,
    samples: usize,
) -> LatencyStats {
    let command = format!(
        "i=0; while [ \"$i\" -lt {samples} ]; do \
         read start _ < /proc/uptime; \
         dd if=/opt/m80-layers/smoke-0/payload.bin of=/dev/null bs=4M status=none; \
         read end _ < /proc/uptime; \
         echo \"$start $end\"; \
         i=$((i + 1)); \
         done"
    );
    let stdout = support::exec_stdout(running, &command);
    let mut values_ms = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(guest_uptime_delta_ms)
        .collect::<Vec<_>>();
    values_ms.sort_by(f64::total_cmp);
    LatencyStats::from_sorted(&values_ms)
}

fn guest_uptime_delta_ms(line: &str) -> f64 {
    let mut fields = line.split_whitespace();
    let start = fields
        .next()
        .unwrap_or_else(|| panic!("guest latency line must contain start uptime: {line:?}"))
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("guest start uptime must be numeric: {line:?}"));
    let end = fields
        .next()
        .unwrap_or_else(|| panic!("guest latency line must contain end uptime: {line:?}"))
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("guest end uptime must be numeric: {line:?}"));
    assert!(
        end >= start,
        "guest uptime must be monotonic in latency line: {line:?}"
    );
    (end - start) * 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_uptime_delta_uses_proc_uptime_fields() {
        let delta = guest_uptime_delta_ms("12.34 12.37");
        assert!((delta - 30.0).abs() < 1e-9);
    }
}

#[derive(Clone, Copy)]
struct LatencyStats {
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
}

impl LatencyStats {
    fn from_sorted(values: &[f64]) -> Self {
        assert!(!values.is_empty(), "latency sample set must be non-empty");
        Self {
            p50_ms: percentile(values, 50.0),
            p95_ms: percentile(values, 95.0),
            p99_ms: percentile(values, 99.0),
        }
    }
}

#[derive(Clone, Copy)]
struct MemInfo {
    mem_available_bytes: u64,
    cached_bytes: u64,
}

struct PressureReport {
    vm_count: usize,
    samples: usize,
    payload_mib: usize,
    digest: String,
    store_path: PathBuf,
    payload_layout: support::PayloadLayout,
    substrate: serde_json::Value,
    git_worktree_dirty: bool,
    git_commit: String,
    pressure_command: String,
    baseline: Vec<LatencyStats>,
    post_pressure: Vec<LatencyStats>,
    mem_before: MemInfo,
    mem_during: MemInfo,
    mem_after: MemInfo,
    leaked_markers: usize,
    leaked_processes: usize,
    leaked_mounts: usize,
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let rank = ((percentile / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn combined_stats(stats: &[LatencyStats], pick: fn(LatencyStats) -> f64) -> f64 {
    let mut values = stats.iter().copied().map(pick).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    percentile(&values, 50.0)
}

fn meminfo() -> MemInfo {
    let raw = std::fs::read_to_string("/proc/meminfo").expect("read /proc/meminfo");
    MemInfo {
        mem_available_bytes: meminfo_kib(&raw, "MemAvailable:") * 1024,
        cached_bytes: meminfo_kib(&raw, "Cached:") * 1024,
    }
}

fn meminfo_kib(raw: &str, key: &str) -> u64 {
    raw.lines()
        .find_map(|line| {
            line.strip_prefix(key).and_then(|rest| {
                rest.split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
            })
        })
        .unwrap_or_else(|| panic!("missing {key} in /proc/meminfo"))
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|raw| {
            raw.parse::<usize>()
                .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}"))
        })
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .map(|raw| {
            raw.parse::<u64>()
                .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}"))
        })
        .unwrap_or(default)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

fn env_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(repo_root().join(path))
    }
}

fn command_output(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("spawn {program}: {err}"));
    if !output.status.success() {
        return format!("{program} {:?} exited {}", args, output.status);
    }
    let raw = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(raw)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn host_filesystem_for(path: &Path) -> String {
    let output = Command::new("findmnt")
        .args(["-T", path.to_str().unwrap(), "-n", "-o", "FSTYPE"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => "unknown".to_owned(),
    }
}

fn process_count_by_basename(names: &[&str]) -> usize {
    let Ok(proc) = std::fs::read_dir("/proc") else {
        return 0;
    };
    proc.filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|raw| raw.parse::<u32>().ok())?;
            std::fs::read(entry.path().join("cmdline")).ok()
        })
        .filter_map(|cmdline| {
            let argv0 = cmdline
                .split(|byte| *byte == 0)
                .find(|part| !part.is_empty())?;
            Some(String::from_utf8_lossy(argv0).into_owned())
        })
        .filter(|program| {
            let basename = Path::new(program)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(program);
            names.iter().any(|name| *name == basename)
        })
        .count()
}

fn mount_count_under(root: &Path) -> usize {
    let root = root.to_string_lossy();
    let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return 0;
    };
    mountinfo
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4))
        .filter(|mountpoint| *mountpoint == root || mountpoint.starts_with(&format!("{root}/")))
        .count()
}

fn write_artifact(path: &Path, report: &PressureReport) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create memory-pressure artifact parent");
    }

    let baseline_p50 = combined_stats(&report.baseline, |stats| stats.p50_ms);
    let baseline_p95 = combined_stats(&report.baseline, |stats| stats.p95_ms);
    let baseline_p99 = combined_stats(&report.baseline, |stats| stats.p99_ms);
    let post_p50 = combined_stats(&report.post_pressure, |stats| stats.p50_ms);
    let post_p95 = combined_stats(&report.post_pressure, |stats| stats.p95_ms);
    let post_p99 = combined_stats(&report.post_pressure, |stats| stats.p99_ms);
    let signal_delta = (post_p50 - baseline_p50).max(0.0);
    let mut out = String::new();

    writeln!(out, "# Pmem DAX Memory Pressure\n").unwrap();
    writeln!(out, "Bead: `m80-q420k.8.9`.\n").unwrap();
    writeln!(out, "## Substrate\n").unwrap();
    writeln!(out, "- substrate kind: `real-kvm`").unwrap();
    writeln!(out, "- commit: `{}`", report.git_commit).unwrap();
    writeln!(
        out,
        "- git worktree dirty excluding this artifact: `{}`",
        report.git_worktree_dirty
    )
    .unwrap();
    writeln!(out, "- host kernel: `{}`", command_output("uname", &["-r"])).unwrap();
    writeln!(
        out,
        "- host filesystem: `{}`",
        host_filesystem_for(Path::new(DEFAULT_STORE_ROOT))
    )
    .unwrap();
    writeln!(
        out,
        "- host memory: `{} bytes MemTotal`",
        meminfo_kib(
            &std::fs::read_to_string("/proc/meminfo").expect("read /proc/meminfo"),
            "MemTotal:"
        ) * 1024
    )
    .unwrap();
    writeln!(out, "- pressure command: `{}`", report.pressure_command).unwrap();
    writeln!(out, "- VM count: `{}`", report.vm_count).unwrap();
    writeln!(out, "- samples per guest: `{}`", report.samples).unwrap();
    writeln!(out, "- payload size: `{} MiB`", report.payload_mib).unwrap();
    writeln!(out, "- image digest: `{}`", report.digest).unwrap();
    writeln!(out, "- image path: `{}`", report.store_path.display()).unwrap();
    writeln!(
        out,
        "- payload erofs layout: `Layout: {}`, size `{}` bytes, on-disk size `{}` bytes, compression ratio `{}`",
        report.payload_layout.layout,
        report.payload_layout.size_bytes,
        report.payload_layout.on_disk_size_bytes,
        report.payload_layout.compression_ratio
    )
    .unwrap();
    let firecracker_bin = std::env::var("M80_FIRECRACKER_BIN")
        .unwrap_or_else(|_| "/opt/firecracker/bin/firecracker".to_owned());
    writeln!(
        out,
        "- Firecracker version: `{}`",
        command_output(&firecracker_bin, &["--version"])
    )
    .unwrap();
    writeln!(
        out,
        "- unrelated VMs running: `{}`",
        report
            .substrate
            .get("allow_other_firecracker_vms")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    )
    .unwrap();
    writeln!(out, "- command:\n").unwrap();
    writeln!(out, "```sh\n{}\n```\n", reproduction_command(path, report)).unwrap();
    writeln!(out, "### Firecracker process substrate\n").unwrap();
    writeln!(
        out,
        "```json\n{}\n```\n",
        serde_json::to_string_pretty(&report.substrate).expect("serialize substrate")
    )
    .unwrap();
    writeln!(out, "## Observable\n").unwrap();
    writeln!(
        out,
        "- baseline Shared-payload read latency p50_ms: `{baseline_p50:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- baseline Shared-payload read latency p95_ms: `{baseline_p95:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- baseline Shared-payload read latency p99_ms: `{baseline_p99:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- post-pressure Shared-payload read latency p50_ms: `{post_p50:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- post-pressure Shared-payload read latency p95_ms: `{post_p95:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- post-pressure Shared-payload read latency p99_ms: `{post_p99:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- cross-guest signal delta p50_ms: `{signal_delta:.3}`"
    )
    .unwrap();
    writeln!(
        out,
        "- host memory delta during pressure bytes: `{}`",
        report
            .mem_before
            .mem_available_bytes
            .saturating_sub(report.mem_during.mem_available_bytes)
    )
    .unwrap();
    writeln!(
        out,
        "- host page-cache delta during pressure bytes: `{}`",
        report
            .mem_before
            .cached_bytes
            .saturating_sub(report.mem_during.cached_bytes)
    )
    .unwrap();
    writeln!(
        out,
        "- host memory delta after refault bytes: `{}`",
        report
            .mem_before
            .mem_available_bytes
            .saturating_sub(report.mem_after.mem_available_bytes)
    )
    .unwrap();
    writeln!(
        out,
        "- host page-cache delta after refault bytes: `{}`",
        report
            .mem_before
            .cached_bytes
            .saturating_sub(report.mem_after.cached_bytes)
    )
    .unwrap();
    writeln!(out, "\n## Teardown Residue\n").unwrap();
    writeln!(out, "- leaked Shared markers: `{}`", report.leaked_markers).unwrap();
    writeln!(
        out,
        "- leaked Firecracker/jailer processes: `{}`",
        report.leaked_processes
    )
    .unwrap();
    writeln!(out, "- leaked mounts: `{}`", report.leaked_mounts).unwrap();
    writeln!(out, "\n## Decision Output\n").unwrap();
    writeln!(
        out,
        "Interpret the signal under the same-trust-domain Shared pmem assumption. \
         If acceptable, update residual-risk docs; otherwise file mitigation beads."
    )
    .unwrap();

    std::fs::write(path, out).expect("write memory-pressure artifact");
}

fn reproduction_command(path: &Path, report: &PressureReport) -> String {
    if let Ok(command) = std::env::var("M80_PMEM_DAX_MEMORY_PRESSURE_REPRO_COMMAND") {
        return command;
    }

    let vm_count = report.vm_count.to_string();
    let samples = report.samples.to_string();
    let payload_mib = report.payload_mib.to_string();
    let artifact = path
        .strip_prefix(repo_root())
        .unwrap_or(path)
        .display()
        .to_string();
    let mut parts = vec![
        "M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1".to_owned(),
        format!(
            "M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND={}",
            shell_quote(&report.pressure_command)
        ),
        format!("M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT={vm_count}"),
        format!("M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES={samples}"),
        format!("M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB={payload_mib}"),
        format!(
            "M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT={}",
            shell_quote(&artifact)
        ),
    ];
    for env_name in ["M80_RUN_ROOT"] {
        if let Ok(value) = std::env::var(env_name) {
            push_env(&mut parts, env_name, &value);
        }
    }
    if let Some(preflight) = report
        .substrate
        .get("preflight_artifacts")
        .and_then(serde_json::Value::as_object)
    {
        for (env_name, field) in [
            ("M80_FIRECRACKER_BIN", "firecracker_bin"),
            ("M80_JAILER_BIN", "jailer_bin"),
            (
                "M80_FIRECRACKER_SECCOMP_FILTER",
                "firecracker_seccomp_filter",
            ),
            ("M80_JAILER_HARDEN_BIN", "jailer_harden_bin"),
            ("M80_NET_HELPER_BIN", "net_helper_bin"),
            ("M80_KERNEL_IMAGE", "kernel_image"),
            ("M80_KERNEL_KIND", "kernel_kind"),
            ("M80_ROOTFS_IMAGE", "rootfs_image"),
        ] {
            if let Some(value) = preflight.get(field).and_then(serde_json::Value::as_str) {
                push_env(&mut parts, env_name, value);
            }
        }
    }
    parts.push(
        "cargo test -p m80-firecracker --test pmem_dax_memory_pressure_real_kvm -- --ignored --nocapture"
            .to_owned(),
    );
    parts.join(" ")
}

fn push_env(parts: &mut Vec<String>, name: &str, value: &str) {
    let prefix = format!("{name}=");
    if !parts.iter().any(|part| part.starts_with(&prefix)) {
        parts.push(format!("{name}={}", shell_quote(value)));
    }
}

fn shell_quote(value: &str) -> String {
    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | ',' | '=')
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
