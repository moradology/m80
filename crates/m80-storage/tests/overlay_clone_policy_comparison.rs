//! Storage-only comparison harness for overlay-template clone policy choices.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use m80_storage::{OverlayTemplateCloneMode, Rootfs};

const DEFAULT_SAMPLES: usize = 50;
const DEFAULT_CONCURRENCY: usize = 8;
const DEFAULT_CONCURRENT_ROUNDS: usize = 20;
const DEFAULT_OVERLAY_SIZE_BYTES: u64 = 64 * 1024 * 1024;

#[test]
#[ignore = "storage benchmark; set M80_RUN_OVERLAY_CLONE_COMPARISON=1"]
fn overlay_clone_policy_comparison() {
    if std::env::var("M80_RUN_OVERLAY_CLONE_COMPARISON").as_deref() != Ok("1") {
        eprintln!("SKIP: set M80_RUN_OVERLAY_CLONE_COMPARISON=1 to write the benchmark artifact");
        return;
    }

    let samples = env_usize("M80_OVERLAY_CLONE_SAMPLES").unwrap_or(DEFAULT_SAMPLES);
    assert!(
        samples >= 30,
        "benchmark requires at least 30 serial samples"
    );
    let concurrency = env_usize("M80_OVERLAY_CLONE_CONCURRENCY").unwrap_or(DEFAULT_CONCURRENCY);
    assert!(concurrency > 0, "concurrency must be non-zero");
    let concurrent_rounds =
        env_usize("M80_OVERLAY_CLONE_CONCURRENT_ROUNDS").unwrap_or(DEFAULT_CONCURRENT_ROUNDS);
    assert!(concurrent_rounds > 0, "concurrent rounds must be non-zero");
    let overlay_size_bytes =
        env_u64("M80_OVERLAY_CLONE_SIZE_BYTES").unwrap_or(DEFAULT_OVERLAY_SIZE_BYTES);
    assert!(overlay_size_bytes > 0, "overlay size must be non-zero");
    let physical_fill_bytes = env_u64("M80_OVERLAY_CLONE_PHYSICAL_FILL_BYTES").unwrap_or(0);
    assert!(
        physical_fill_bytes <= overlay_size_bytes,
        "physical fill bytes must fit inside overlay size"
    );

    let repo = repo_root();
    let artifact = env_path("M80_OVERLAY_CLONE_ARTIFACT")
        .unwrap_or_else(|| repo.join("docs/perf/overlay-clone-policy-comparison.md"));
    let run_root = env_path("M80_OVERLAY_CLONE_RUN_ROOT").unwrap_or_else(|| {
        PathBuf::from(format!(
            "/var/tmp/m80-overlay-clone-policy-comparison-{}",
            std::process::id()
        ))
    });
    assert!(
        !run_root.exists(),
        "run root already exists; choose a fresh M80_OVERLAY_CLONE_RUN_ROOT: {}",
        run_root.display()
    );

    std::fs::create_dir(&run_root).expect("create run root");
    let fs = findmnt(&run_root);
    let base = run_root.join("base.ext4");
    std::fs::write(&base, b"fake-base\n").expect("base rootfs placeholder");
    let template = run_root.join("template.ext4");
    create_sparse_ext4_template(&template, overlay_size_bytes, physical_fill_bytes);
    let template_allocated = allocated_bytes(&template).expect("template allocated bytes");
    let reflink_probe = probe_reflink_always(&template, &run_root.join("reflink-probe.ext4"));

    let current_rootfs_prepare =
        measure_current_rootfs_prepare(&run_root, &base, samples, overlay_size_bytes);
    let byte_copy_sparse_always = measure_cp_cell(
        "byte_copy_sparse_always",
        &template,
        &run_root,
        samples,
        &["--reflink=never", "--sparse=always"],
    );
    let byte_copy_sparse_auto = measure_cp_cell(
        "byte_copy_sparse_auto",
        &template,
        &run_root,
        samples,
        &["--reflink=never", "--sparse=auto"],
    );
    let reflink_always = if reflink_probe.supported {
        Some(measure_cp_cell(
            "reflink_always",
            &template,
            &run_root,
            samples,
            &["--reflink=always", "--sparse=auto"],
        ))
    } else {
        None
    };
    let reflink_auto = measure_cp_cell(
        "reflink_auto",
        &template,
        &run_root,
        samples,
        &["--reflink=auto", "--sparse=auto"],
    );
    let concurrent_byte_copy = measure_concurrent_byte_copy(
        &template,
        &run_root,
        concurrency,
        concurrent_rounds,
        &["--reflink=never", "--sparse=always"],
    );

    let mount_count_before_teardown = mount_count_under(&run_root);
    std::fs::remove_dir_all(&run_root).expect("remove run root");
    let run_root_removed = !run_root.exists();

    let doc = render_artifact(Artifact {
        samples,
        concurrency,
        concurrent_rounds,
        run_root: run_root.clone(),
        artifact: artifact.clone(),
        fs,
        template_allocated,
        reflink_probe,
        current_rootfs_prepare,
        byte_copy_sparse_always,
        byte_copy_sparse_auto,
        reflink_always,
        reflink_auto,
        concurrent_byte_copy,
        overlay_size_bytes,
        physical_fill_bytes,
        mount_count_before_teardown,
        run_root_removed,
        git_commit: git_rev_parse("HEAD"),
        git_worktree_dirty: git_worktree_dirty_excluding(&artifact),
        kernel: command_stdout("uname", &["-r"]).unwrap_or_else(|| "unknown".to_owned()),
    });
    if let Some(parent) = artifact.parent() {
        std::fs::create_dir_all(parent).expect("artifact parent");
    }
    std::fs::write(&artifact, doc).expect("write artifact");
    eprintln!("M80_OVERLAY_CLONE_ARTIFACT {}", artifact.display());
}

fn measure_current_rootfs_prepare(
    run_root: &Path,
    base: &Path,
    samples: usize,
    overlay_size_bytes: u64,
) -> Cell {
    let prime_dir = run_root.join("current-prime");
    std::fs::create_dir(&prime_dir).expect("prime dir");
    let prime_overlay = prime_dir.join("rootfs.overlay.ext4");
    Rootfs::prepare(
        base,
        &prime_overlay,
        overlay_size_bytes,
        OverlayTemplateCloneMode::ByteCopy,
    )
    .expect("prime Rootfs::prepare");
    std::fs::remove_file(&prime_overlay).expect("remove prime overlay");
    std::fs::remove_dir(&prime_dir).expect("remove prime dir");

    let mut samples_ms = Vec::with_capacity(samples);
    let mut allocated = Vec::with_capacity(samples);
    for idx in 0..samples {
        let vm_dir = run_root.join(format!("current-{idx:03}"));
        std::fs::create_dir(&vm_dir).expect("vm dir");
        let overlay = vm_dir.join("rootfs.overlay.ext4");
        let started = Instant::now();
        Rootfs::prepare(
            base,
            &overlay,
            overlay_size_bytes,
            OverlayTemplateCloneMode::ByteCopy,
        )
        .expect("Rootfs::prepare");
        samples_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        allocated.push(allocated_bytes(&overlay).expect("overlay allocated bytes"));
        std::fs::remove_file(&overlay).expect("remove overlay");
        std::fs::remove_dir(&vm_dir).expect("remove vm dir");
    }
    Cell::new("current_rootfs_prepare", samples_ms, allocated)
}

fn measure_cp_cell(
    name: &'static str,
    template: &Path,
    run_root: &Path,
    samples: usize,
    args: &[&str],
) -> Cell {
    let mut samples_ms = Vec::with_capacity(samples);
    let mut allocated = Vec::with_capacity(samples);
    for idx in 0..samples {
        let dest = run_root.join(format!("{name}-{idx:03}.ext4"));
        let started = Instant::now();
        run_cp(template, &dest, args);
        samples_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        allocated.push(allocated_bytes(&dest).expect("dest allocated bytes"));
        std::fs::remove_file(&dest).expect("remove dest");
    }
    Cell::new(name, samples_ms, allocated)
}

fn measure_concurrent_byte_copy(
    template: &Path,
    run_root: &Path,
    concurrency: usize,
    rounds: usize,
    args: &[&str],
) -> ConcurrentCell {
    let mut per_clone_ms = Vec::with_capacity(concurrency * rounds);
    let mut round_wall_ms = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let round_dir = run_root.join(format!("concurrent-{round:03}"));
        std::fs::create_dir(&round_dir).expect("round dir");
        let started_round = Instant::now();
        let (sender, receiver) = mpsc::channel();
        let mut handles = Vec::with_capacity(concurrency);
        for worker in 0..concurrency {
            let sender = sender.clone();
            let template = template.to_path_buf();
            let dest = round_dir.join(format!("worker-{worker:03}.ext4"));
            let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
            handles.push(thread::spawn(move || {
                let started = Instant::now();
                run_cp_owned(&template, &dest, &args);
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                let allocated = allocated_bytes(&dest).expect("dest allocated bytes");
                std::fs::remove_file(&dest).expect("remove dest");
                sender.send((elapsed, allocated)).expect("send result");
            }));
        }
        drop(sender);
        for handle in handles {
            handle.join().expect("copy thread must not panic");
        }
        round_wall_ms.push(started_round.elapsed().as_secs_f64() * 1000.0);
        for (elapsed, _allocated) in receiver {
            per_clone_ms.push(elapsed);
        }
        std::fs::remove_dir(&round_dir).expect("remove round dir");
    }
    per_clone_ms.sort_by(f64::total_cmp);
    round_wall_ms.sort_by(f64::total_cmp);
    ConcurrentCell {
        name: "concurrent_byte_copy",
        total_clones: concurrency * rounds,
        concurrency,
        rounds,
        per_clone: Stats::from_sorted(&per_clone_ms),
        round_wall: Stats::from_sorted(&round_wall_ms),
    }
}

fn run_cp(template: &Path, dest: &Path, args: &[&str]) {
    let output = Command::new("cp")
        .args(args)
        .arg(template)
        .arg(dest)
        .output()
        .expect("spawn cp");
    assert!(
        output.status.success(),
        "cp failed: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_cp_owned(template: &Path, dest: &Path, args: &[String]) {
    let output = Command::new("cp")
        .args(args)
        .arg(template)
        .arg(dest)
        .output()
        .expect("spawn cp");
    assert!(
        output.status.success(),
        "cp failed: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_sparse_ext4_template(path: &Path, overlay_size_bytes: u64, physical_fill_bytes: u64) {
    let file = std::fs::File::create(path).expect("create template");
    file.set_len(overlay_size_bytes).expect("set template size");
    drop(file);
    run_checked("mkfs.ext4", &["-F", path.to_str().expect("utf8 path")]);
    run_checked("fallocate", &["-d", path.to_str().expect("utf8 path")]);
    if physical_fill_bytes > 0 {
        fill_with_deterministic_noise(path, physical_fill_bytes);
    }
}

fn fill_with_deterministic_noise(path: &Path, bytes: u64) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open template for noise fill");
    let mut remaining = bytes;
    let mut state = 0x4d38305f_6e6f6973_u64;
    let mut buf = vec![0_u8; 1024 * 1024];
    while remaining > 0 {
        for chunk in buf.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.copy_from_slice(&state.to_le_bytes());
        }
        let n = remaining.min(buf.len() as u64) as usize;
        file.write_all(&buf[..n]).expect("write noise");
        remaining -= n as u64;
    }
    file.sync_all().expect("sync noise-filled template");
}

fn run_checked(program: &str, args: &[&str]) {
    let output = Command::new(program)
        .args(args)
        .output()
        .expect("spawn helper");
    assert!(
        output.status.success(),
        "{program} failed: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[derive(Debug, Clone)]
struct ProbeResult {
    supported: bool,
    detail: String,
}

fn probe_reflink_always(template: &Path, dest: &Path) -> ProbeResult {
    let output = Command::new("cp")
        .args(["--reflink=always", "--sparse=auto"])
        .arg(template)
        .arg(dest)
        .output()
        .expect("spawn reflink probe");
    let detail = if output.status.success() {
        std::fs::remove_file(dest).expect("remove reflink probe dest");
        "supported".to_owned()
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            format!("unsupported: cp exited {:?}", output.status.code())
        } else {
            format!("unsupported: {stderr}")
        }
    };
    ProbeResult {
        supported: output.status.success(),
        detail,
    }
}

#[derive(Debug, Clone)]
struct Cell {
    name: &'static str,
    stats: Stats,
    allocated_p50: u64,
}

impl Cell {
    fn new(name: &'static str, mut samples_ms: Vec<f64>, mut allocated: Vec<u64>) -> Self {
        samples_ms.sort_by(f64::total_cmp);
        allocated.sort_unstable();
        let allocated_p50 = allocated[allocated.len() / 2];
        Self {
            name,
            stats: Stats::from_sorted(&samples_ms),
            allocated_p50,
        }
    }
}

#[derive(Debug, Clone)]
struct ConcurrentCell {
    name: &'static str,
    total_clones: usize,
    concurrency: usize,
    rounds: usize,
    per_clone: Stats,
    round_wall: Stats,
}

#[derive(Debug, Clone, Copy)]
struct Stats {
    p50: f64,
    p95: f64,
    p99: f64,
    min: f64,
    max: f64,
}

impl Stats {
    fn from_sorted(values: &[f64]) -> Self {
        assert!(!values.is_empty());
        Self {
            p50: percentile(values, 50.0),
            p95: percentile(values, 95.0),
            p99: percentile(values, 99.0),
            min: values[0],
            max: values[values.len() - 1],
        }
    }
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let rank = ((percentile / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Debug, Clone)]
struct FsInfo {
    target: String,
    fstype: String,
    source: String,
    options: String,
}

fn findmnt(path: &Path) -> FsInfo {
    let output = Command::new("findmnt")
        .args([
            "-T",
            path.to_str().expect("utf8 path"),
            "-n",
            "-o",
            "TARGET,FSTYPE,SOURCE,OPTIONS",
        ])
        .output()
        .expect("spawn findmnt");
    assert!(
        output.status.success(),
        "findmnt failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("findmnt utf8");
    let mut parts = stdout.trim().splitn(4, char::is_whitespace);
    FsInfo {
        target: parts.next().unwrap_or_default().to_owned(),
        fstype: parts.next().unwrap_or_default().to_owned(),
        source: parts.next().unwrap_or_default().to_owned(),
        options: parts.next().unwrap_or_default().trim().to_owned(),
    }
}

fn allocated_bytes(path: &Path) -> std::io::Result<u64> {
    let output = Command::new("du")
        .args(["-B1", path.to_str().unwrap()])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other("du failed"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let bytes = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| std::io::Error::other("du output missing byte count"))?
        .parse()
        .map_err(|_| std::io::Error::other("du byte count parse failed"))?;
    Ok(bytes)
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

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.parse().ok()
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.parse().ok()
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}

fn git_rev_parse(rev: &str) -> String {
    command_stdout(
        "git",
        &["-C", &repo_root().display().to_string(), "rev-parse", rev],
    )
    .unwrap_or_else(|| "unknown".to_owned())
}

fn git_worktree_dirty_excluding(path: &Path) -> bool {
    let repo = repo_root();
    let rel = path
        .strip_prefix(&repo)
        .ok()
        .and_then(Path::to_str)
        .unwrap_or_default()
        .to_owned();
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

struct Artifact {
    samples: usize,
    concurrency: usize,
    concurrent_rounds: usize,
    run_root: PathBuf,
    artifact: PathBuf,
    fs: FsInfo,
    template_allocated: u64,
    reflink_probe: ProbeResult,
    current_rootfs_prepare: Cell,
    byte_copy_sparse_always: Cell,
    byte_copy_sparse_auto: Cell,
    reflink_always: Option<Cell>,
    reflink_auto: Cell,
    concurrent_byte_copy: ConcurrentCell,
    overlay_size_bytes: u64,
    physical_fill_bytes: u64,
    mount_count_before_teardown: usize,
    run_root_removed: bool,
    git_commit: String,
    git_worktree_dirty: bool,
    kernel: String,
}

fn render_artifact(data: Artifact) -> String {
    let reflink_row = match &data.reflink_always {
        Some(cell) => serial_row(cell),
        None => format!(
            "| reflink_always | unsupported | unsupported | unsupported | unsupported | unsupported | {} |\n",
            data.reflink_probe.detail.replace('|', "\\|")
        ),
    };
    format!(
        r#"# Overlay Clone Policy Comparison

## Substrate

- host filesystem: `{}` mounted at `{}` from `{}`
- filesystem options: `{}`
- kernel: `{}`
- run root: `{}`
- artifact: `{}`
- commit: `{}`
- git worktree dirty excluding this artifact: `{}`
- substrate kind: `storage-only`
- overlay size bytes: `{}`
- template physical fill bytes: `{}`
- template allocated bytes after hole digging: `{}`
- serial samples: `{}`
- concurrent byte-copy: `{}` rounds at concurrency `{}`
- reflink probe: `{}`

## Serial Clone Results

| cell | p50_ms | p95_ms | p99_ms | min_ms | max_ms | allocated_p50_bytes |
|---|---:|---:|---:|---:|---:|---:|
{}{}{}{}{}

## Concurrent Byte-Copy Results

| cell | clones | concurrency | rounds | per_clone_p50_ms | per_clone_p95_ms | per_clone_p99_ms | round_wall_p50_ms | round_wall_p95_ms | round_wall_p99_ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| {} | {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |

## Residue

- mount entries under run root before teardown: `{}`
- run root removed: `{}`

## Interpretation

This artifact compares explicit overlay-template clone commands without changing
production behavior. `current_rootfs_prepare` includes m80's current template
validation, explicit byte-copy selection, and `Rootfs::prepare` call overhead.
When `template physical fill bytes` is non-zero, `current_rootfs_prepare`
remains m80's current sparse-template control; the explicit `cp` rows copy the
noise-filled template. `byte_copy_sparse_auto` is m80's current explicit
byte-copy command. `byte_copy_sparse_always` is retained as a historical
comparison against forced sparse scanning. `reflink_auto` is included only as a
GNU cp auto-degrade reference; it is not the desired explicit policy model.

For empty sparse templates, explicit byte-copy below 20 ms P50 / 40 ms P95
makes deleting the implicit fallback machinery performance-plausible on this
host.
When `template physical fill bytes` is non-zero, this artifact is a copy-scaling
reference rather than an empty-template policy gate.
"#,
        data.fs.fstype,
        data.fs.target,
        data.fs.source,
        data.fs.options,
        data.kernel,
        data.run_root.display(),
        data.artifact.display(),
        data.git_commit,
        data.git_worktree_dirty,
        data.overlay_size_bytes,
        data.physical_fill_bytes,
        data.template_allocated,
        data.samples,
        data.concurrent_rounds,
        data.concurrency,
        data.reflink_probe.detail,
        serial_row(&data.current_rootfs_prepare),
        serial_row(&data.byte_copy_sparse_always),
        serial_row(&data.byte_copy_sparse_auto),
        reflink_row,
        serial_row(&data.reflink_auto),
        data.concurrent_byte_copy.name,
        data.concurrent_byte_copy.total_clones,
        data.concurrent_byte_copy.concurrency,
        data.concurrent_byte_copy.rounds,
        data.concurrent_byte_copy.per_clone.p50,
        data.concurrent_byte_copy.per_clone.p95,
        data.concurrent_byte_copy.per_clone.p99,
        data.concurrent_byte_copy.round_wall.p50,
        data.concurrent_byte_copy.round_wall.p95,
        data.concurrent_byte_copy.round_wall.p99,
        data.mount_count_before_teardown,
        data.run_root_removed
    )
}

fn serial_row(cell: &Cell) -> String {
    format!(
        "| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {} |\n",
        cell.name,
        cell.stats.p50,
        cell.stats.p95,
        cell.stats.p99,
        cell.stats.min,
        cell.stats.max,
        cell.allocated_p50
    )
}
