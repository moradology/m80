//! Scenario 2: host-memory density proof for Shared pmem.
//!
//! This measurement-shaped scenario is owned by `m80-q420k.3.8`. The final
//! verified-close proof must come from `scripts/smoke-pmem-shared.sh` and the
//! committed numeric artifact at `docs/perf/pmem-shared-density.md`, not from a
//! mock or `--no-run` result.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
mod pmem_shared_support;
#[path = "e2e_composed_real_kvm/quiet_host.rs"]
mod quiet_host;

use pmem_shared_support as support;

#[test]
#[ignore = "measurement-shaped; m80-q420k.3.8 owns the verified density run"]
fn shared_pmem_host_page_sharing_measurement_lives_in_density_gate() {
    if std::env::var_os("M80_RUN_PMEM_SHARED_DENSITY").is_none() {
        eprintln!("skipping density run; run scripts/smoke-pmem-shared.sh for m80-q420k.3.8");
        return;
    }

    let _serial = support::REAL_KVM_LOCK.lock().expect("real-kvm test lock");
    let (allow_other_firecracker_vms, preexisting_firecrackers) =
        quiet_host::assert_or_record_quiet_host_for(
            "pmem Shared density",
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS",
        );
    let mut substrate =
        quiet_host::substrate_json(allow_other_firecracker_vms, &preexisting_firecrackers);
    let vm_count = env_usize("M80_PMEM_SHARED_VM_COUNT", 4);
    let cycles = env_usize("M80_PMEM_SHARED_CYCLES", 10);
    let payload_mib = env_usize("M80_PMEM_SHARED_PAYLOAD_MIB", 128);
    let per_vm_overhead_kib = env_u64("M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB", 128 * 1024);
    assert!(vm_count >= 4, "M80_PMEM_SHARED_VM_COUNT must be >= 4");
    assert!(cycles >= 1, "M80_PMEM_SHARED_CYCLES must be >= 1");

    let store = support::open_default_store();
    let digest = support::build_payload_image_digest(&store, payload_mib);
    let store_path = support::store_erofs_path(&store, &digest);
    let payload_layout =
        support::assert_payload_file_uncompressed_non_inlined(&store_path, "payload.bin");
    let image_bytes = std::fs::metadata(&store_path)
        .expect("density erofs metadata")
        .len();
    let image_kib = image_bytes.div_ceil(1024);
    let bound_kib = image_kib + per_vm_overhead_kib * vm_count as u64;
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    quiet_host::record_preflight_artifacts(&mut substrate, &discovery);
    let real = support::real_backend_from_discovery(discovery, vm_count as u32);
    let mut samples = Vec::with_capacity(cycles);
    let mut max_active_ref_count = 0;
    let mut final_shared_ref_count = 0;
    let mut stale_marker_sweep_count = 0;
    let mut canonical_artifact_present_after_teardown = false;
    let git_commit = quiet_host::git_head_commit();

    for cycle in 1..=cycles {
        drop_host_caches();
        let before_kib = mem_available_kib();
        let mut running = Vec::with_capacity(vm_count);
        for idx in 0..vm_count {
            let mut sandbox = support::launch_with_layers(
                &real.backend,
                &format!("sd{cycle}-{idx}"),
                vec![support::shared_layer(&digest, 0)],
            );
            support::assert_one_pmem_mount(&mut sandbox, 0);
            running.push(sandbox);
        }
        let active_ref_count = store.shared_ref_count(&digest).unwrap();
        assert_eq!(
            active_ref_count, vm_count,
            "all density VMs must hold Shared active-use markers"
        );
        max_active_ref_count = max_active_ref_count.max(active_ref_count);

        for sandbox in &mut running {
            support::exec_stdout(
                sandbox,
                "dd if=/opt/m80-layers/smoke-0/payload.bin of=/dev/null bs=4M status=none",
            );
        }
        let after_kib = mem_available_kib();
        let delta_kib = before_kib.saturating_sub(after_kib);

        while let Some(sandbox) = running.pop() {
            support::stop_and_delete(sandbox);
        }
        final_shared_ref_count = store.shared_ref_count(&digest).unwrap();
        assert_eq!(
            final_shared_ref_count, 0,
            "density teardown must release all Shared markers"
        );
        stale_marker_sweep_count = store
            .sweep_shared_refs(std::iter::empty::<&str>())
            .expect("post-density stale marker sweep");
        assert_eq!(
            stale_marker_sweep_count, 0,
            "density teardown should leave no stale Shared markers"
        );
        canonical_artifact_present_after_teardown = store_path.is_file();
        assert!(
            canonical_artifact_present_after_teardown,
            "canonical Shared artifact must remain after density cycle"
        );
        assert!(
            delta_kib <= bound_kib,
            "density cycle {cycle} exceeded bound: delta_kib={delta_kib} bound_kib={bound_kib}"
        );

        println!(
            "M80_PMEM_SHARED_DENSITY cycle={cycle} vm_count={vm_count} before_kib={before_kib} after_kib={after_kib} delta_kib={delta_kib} bound_kib={bound_kib} image_bytes={image_bytes} digest={}",
            digest.as_str()
        );
        samples.push(Sample {
            cycle,
            before_kib,
            after_kib,
            delta_kib,
        });
    }
    let post_run_firecrackers = quiet_host::firecracker_processes();
    quiet_host::record_post_run_firecracker_processes(&mut substrate, &post_run_firecrackers);

    if let Some(path) = std::env::var_os("M80_PMEM_SHARED_DENSITY_ARTIFACT") {
        let path = PathBuf::from(path);
        let git_worktree_dirty_excluding_artifact =
            quiet_host::git_worktree_dirty_excluding(&[path.clone()]);
        write_density_artifact(
            &path,
            &DensityReport {
                vm_count,
                cycles,
                payload_mib,
                per_vm_overhead_kib,
                image_bytes,
                image_kib,
                bound_kib,
                payload_layout,
                substrate,
                git_worktree_dirty_excluding_artifact,
                git_commit,
                max_active_ref_count,
                final_shared_ref_count,
                stale_marker_sweep_count,
                canonical_artifact_present_after_teardown,
                digest: digest.as_str().to_owned(),
                store_path,
                samples,
            },
        );
    }
}

struct Sample {
    cycle: usize,
    before_kib: u64,
    after_kib: u64,
    delta_kib: u64,
}

struct DensityReport {
    vm_count: usize,
    cycles: usize,
    payload_mib: usize,
    per_vm_overhead_kib: u64,
    image_bytes: u64,
    image_kib: u64,
    bound_kib: u64,
    payload_layout: support::PayloadLayout,
    substrate: serde_json::Value,
    git_worktree_dirty_excluding_artifact: bool,
    git_commit: String,
    max_active_ref_count: usize,
    final_shared_ref_count: usize,
    stale_marker_sweep_count: usize,
    canonical_artifact_present_after_teardown: bool,
    digest: String,
    store_path: PathBuf,
    samples: Vec<Sample>,
}

fn env_usize(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}")),
        Err(_) => default,
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("{name} must be a positive integer, got {raw:?}")),
        Err(_) => default,
    }
}

fn mem_available_kib() -> u64 {
    let meminfo = std::fs::read_to_string("/proc/meminfo").expect("read /proc/meminfo");
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            return rest
                .split_whitespace()
                .next()
                .expect("MemAvailable value")
                .parse::<u64>()
                .expect("MemAvailable value is integer KiB");
        }
    }
    panic!("MemAvailable not found in /proc/meminfo");
}

fn drop_host_caches() {
    Command::new("sync")
        .status()
        .expect("spawn sync")
        .success()
        .then_some(())
        .expect("sync succeeded");
    std::fs::write("/proc/sys/vm/drop_caches", b"3\n").expect("drop host page cache");
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

fn write_density_artifact(path: &Path, report: &DensityReport) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create density artifact parent");
    }
    let max_delta = report
        .samples
        .iter()
        .map(|sample| sample.delta_kib)
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    writeln!(out, "# Shared pmem density\n").unwrap();
    writeln!(out, "Generated: `{}`\n", command_output("date", &["-Is"])).unwrap();
    writeln!(out, "## Reproduction\n").unwrap();
    writeln!(
        out,
        "Command: `{}`\n",
        std::env::var("M80_PMEM_SHARED_REPRO_COMMAND")
            .unwrap_or_else(|_| "scripts/smoke-pmem-shared.sh".to_owned())
    )
    .unwrap();
    writeln!(out, "## Substrate\n").unwrap();
    writeln!(out, "- host kernel: `{}`", command_output("uname", &["-r"])).unwrap();
    let firecracker_bin = std::env::var("M80_FIRECRACKER_BIN")
        .unwrap_or_else(|_| "/opt/firecracker/bin/firecracker".to_owned());
    writeln!(
        out,
        "- firecracker: `{}`",
        command_output(&firecracker_bin, &["--version"])
    )
    .unwrap();
    writeln!(
        out,
        "- `/dev/kvm`: `{}`",
        command_output("stat", &["-c", "%A %U:%G %n", "/dev/kvm"])
    )
    .unwrap();
    writeln!(
        out,
        "- sudo: required; test ran as uid `{}`",
        command_output("id", &["-u"])
    )
    .unwrap();
    writeln!(
        out,
        "- dropped page cache before each cycle: `sync && echo 3 > /proc/sys/vm/drop_caches`"
    )
    .unwrap();
    writeln!(
        out,
        "- quiet host check: `{}`",
        report
            .substrate
            .get("quiet_host_checked")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    )
    .unwrap();
    writeln!(
        out,
        "- allow other Firecracker VMs: `{}`",
        report
            .substrate
            .get("allow_other_firecracker_vms")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    )
    .unwrap();
    writeln!(
        out,
        "- git worktree dirty excluding this artifact: `{}`",
        report.git_worktree_dirty_excluding_artifact
    )
    .unwrap();
    writeln!(out, "- git commit: `{}`", report.git_commit).unwrap();
    writeln!(out, "\n### Firecracker process substrate\n").unwrap();
    writeln!(
        out,
        "```json\n{}\n```\n",
        serde_json::to_string_pretty(&report.substrate).expect("serialize substrate")
    )
    .unwrap();
    writeln!(out, "## Observable\n").unwrap();
    writeln!(
        out,
        "- field: host memory delta after {} attached Shared VMs",
        report.vm_count
    )
    .unwrap();
    writeln!(out, "- cycles: `{}`", report.cycles).unwrap();
    writeln!(out, "- payload size: `{} MiB`", report.payload_mib).unwrap();
    writeln!(out, "- image digest: `{}`", report.digest).unwrap();
    writeln!(out, "- image path: `{}`", report.store_path.display()).unwrap();
    writeln!(out, "- image bytes: `{}`", report.image_bytes).unwrap();
    writeln!(out, "- image KiB: `{}`", report.image_kib).unwrap();
    writeln!(
        out,
        "- payload erofs layout: `Layout: {}`, size `{}` bytes, on-disk size `{}` bytes, compression ratio `{}`",
        report.payload_layout.layout,
        report.payload_layout.size_bytes,
        report.payload_layout.on_disk_size_bytes,
        report.payload_layout.compression_ratio
    )
    .unwrap();
    writeln!(
        out,
        "- per-VM overhead bound: `{} KiB`",
        report.per_vm_overhead_kib
    )
    .unwrap();
    writeln!(out, "- bound: `{} KiB`", report.bound_kib).unwrap();
    writeln!(out, "- max observed delta: `{max_delta} KiB`").unwrap();
    writeln!(
        out,
        "- result: `{}`\n",
        if max_delta <= report.bound_kib {
            "pass"
        } else {
            "fail"
        }
    )
    .unwrap();
    writeln!(out, "## Teardown\n").unwrap();
    writeln!(
        out,
        "- max active-use markers observed: `{}`",
        report.max_active_ref_count
    )
    .unwrap();
    writeln!(
        out,
        "- final active-use markers: `{}`",
        report.final_shared_ref_count
    )
    .unwrap();
    writeln!(
        out,
        "- stale markers swept after teardown: `{}`",
        report.stale_marker_sweep_count
    )
    .unwrap();
    writeln!(
        out,
        "- canonical Shared artifact present after teardown: `{}`\n",
        report.canonical_artifact_present_after_teardown
    )
    .unwrap();
    writeln!(out, "## Samples\n").unwrap();
    writeln!(
        out,
        "| cycle | MemAvailable before KiB | MemAvailable after KiB | delta KiB | bound KiB |"
    )
    .unwrap();
    writeln!(out, "|---:|---:|---:|---:|---:|").unwrap();
    for sample in &report.samples {
        writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            sample.cycle, sample.before_kib, sample.after_kib, sample.delta_kib, report.bound_kib
        )
        .unwrap();
    }
    writeln!(out, "\n## Payload erofs layout\n").unwrap();
    writeln!(
        out,
        "The measured file is required to match the `.8.12` file-level DAX result: uncompressed, non-inlined erofs layout 0.\n"
    )
    .unwrap();
    writeln!(out, "```text\n{}```\n", report.payload_layout.raw_dump).unwrap();
    writeln!(out, "## Trust model\n").unwrap();
    writeln!(
        out,
        "Shared pmem is admitted only with `TrustDomainAck` in the same trust domain."
    )
    .unwrap();
    writeln!(
        out,
        "The DAX cache-timing side channel is acknowledged by that trust model."
    )
    .unwrap();
    writeln!(
        out,
        "Shared pmem jail bindings are read-only; writable layers must use `PmemSharing::PerVm`.\n"
    )
    .unwrap();
    std::fs::write(path, out).expect("write density artifact");
    println!("M80_PMEM_SHARED_DENSITY_ARTIFACT {}", path.display());
}
