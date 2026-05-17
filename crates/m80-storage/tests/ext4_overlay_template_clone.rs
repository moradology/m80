//! Measurement harness for ext4 overlay-template byte-copy fallback.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use m80_storage::Rootfs;

const DEFAULT_SAMPLES: usize = 30;
const OVERLAY_SIZE_BYTES: u64 = 64 * 1024 * 1024;

#[test]
#[ignore = "measurement harness for m80-q420k.8.16; set M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1"]
fn ext4_overlay_template_clone_measurement() {
    if std::env::var("M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE").as_deref() != Ok("1") {
        eprintln!(
            "SKIP: set M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1 to write the measurement artifact"
        );
        return;
    }

    let samples = env_usize("M80_EXT4_OVERLAY_TEMPLATE_SAMPLES").unwrap_or(DEFAULT_SAMPLES);
    assert!(samples >= 30, "measurement close requires N >= 30 samples");

    let repo = repo_root();
    let artifact = env_path("M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT")
        .unwrap_or_else(|| repo.join("docs/perf/ext4-overlay-template-clone.md"));
    let run_root = env_path("M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT").unwrap_or_else(|| {
        PathBuf::from(format!(
            "/var/tmp/m80-ext4-overlay-template-clone-{}",
            std::process::id()
        ))
    });
    assert!(
        !run_root.exists(),
        "run root already exists; choose a fresh M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT: {}",
        run_root.display()
    );

    std::fs::create_dir(&run_root).expect("create run root");
    let fs = findmnt_fstype(&run_root);
    assert_eq!(fs.fstype, "ext4", "measurement run root must be ext4");

    let dm_before = dmsetup_device_count();
    let mounts_before = mount_count_under(&run_root);
    let base = run_root.join("base.ext4");
    std::fs::write(&base, b"fake-base\n").expect("base rootfs placeholder");

    let prime_dir = run_root.join("vm-prime");
    std::fs::create_dir(&prime_dir).expect("prime vm dir");
    let prime_overlay = prime_dir.join("rootfs.overlay.ext4");
    Rootfs::prepare(&base, &prime_overlay, OVERLAY_SIZE_BYTES).expect("prime template");
    std::fs::remove_file(&prime_overlay).expect("remove prime overlay");
    std::fs::remove_dir(&prime_dir).expect("remove prime dir");

    let mut durations_ms = Vec::with_capacity(samples);
    for idx in 0..samples {
        let vm_dir = run_root.join(format!("vm-{idx:03}"));
        std::fs::create_dir(&vm_dir).expect("vm dir");
        let overlay = vm_dir.join("rootfs.overlay.ext4");
        let started = Instant::now();
        Rootfs::prepare(&base, &overlay, OVERLAY_SIZE_BYTES).expect("prepare overlay");
        durations_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        std::fs::remove_file(&overlay).expect("remove overlay");
        std::fs::remove_dir(&vm_dir).expect("remove vm dir");
    }

    let template = run_root.join(format!(
        ".rootfs-overlay-template-v1-{OVERLAY_SIZE_BYTES}.ext4"
    ));
    let template_size = std::fs::metadata(&template)
        .expect("template metadata")
        .len();
    let template_allocated = allocated_bytes(&template).expect("template allocated bytes");
    let dm_after = dmsetup_device_count();
    let mounts_after = mount_count_under(&run_root);
    std::fs::remove_dir_all(&run_root).expect("remove run root");
    let run_root_removed = !run_root.exists();

    durations_ms.sort_by(f64::total_cmp);
    let stats = Stats::from_sorted(&durations_ms);
    let dm_leaked = match (dm_before, dm_after) {
        (Some(before), Some(after)) => after.saturating_sub(before),
        _ => 0,
    };
    let mounts_leaked = mounts_after.saturating_sub(mounts_before);

    let doc = render_artifact(Artifact {
        samples,
        run_root: &run_root,
        artifact: &artifact,
        command: measurement_command(samples, &run_root, &artifact),
        fs: &fs,
        template_size,
        template_allocated,
        stats,
        dm_before,
        dm_after,
        dm_leaked,
        mounts_before,
        mounts_after,
        mounts_leaked,
        run_root_removed,
        git_commit: git_rev_parse("HEAD"),
        git_worktree_dirty: git_worktree_dirty_excluding(&artifact),
        kernel: command_stdout("uname", &["-r"]).unwrap_or_else(|| "unknown".to_owned()),
    });
    if let Some(parent) = artifact.parent() {
        std::fs::create_dir_all(parent).expect("artifact parent");
    }
    std::fs::write(&artifact, doc).expect("write artifact");
    eprintln!("M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT {}", artifact.display());
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

#[derive(Debug)]
struct FsInfo {
    target: String,
    fstype: String,
    source: String,
    options: String,
}

fn findmnt_fstype(path: &Path) -> FsInfo {
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

fn dmsetup_device_count() -> Option<usize> {
    let output = Command::new(dmsetup_bin())
        .args(["ls", "--noheadings"])
        .output()
        .ok()?;
    if !output.status.success() {
        return sysfs_dm_device_count();
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(
        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
    )
}

fn dmsetup_bin() -> &'static str {
    if Path::new("/usr/sbin/dmsetup").exists() {
        "/usr/sbin/dmsetup"
    } else {
        "dmsetup"
    }
}

fn sysfs_dm_device_count() -> Option<usize> {
    let entries = std::fs::read_dir("/sys/block").ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("dm-"))
            .count(),
    )
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
        .and_then(|path| path.to_str())
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

struct Artifact<'a> {
    samples: usize,
    run_root: &'a Path,
    artifact: &'a Path,
    command: String,
    fs: &'a FsInfo,
    template_size: u64,
    template_allocated: u64,
    stats: Stats,
    dm_before: Option<usize>,
    dm_after: Option<usize>,
    dm_leaked: usize,
    mounts_before: usize,
    mounts_after: usize,
    mounts_leaked: usize,
    run_root_removed: bool,
    git_commit: String,
    git_worktree_dirty: bool,
    kernel: String,
}

fn render_artifact(data: Artifact<'_>) -> String {
    format!(
        r#"# Ext4 Overlay-Template Clone Measurement

Bead: `m80-q420k.8.16`.

## Substrate

- host filesystem: `{}` mounted at `{}` from `{}`
- filesystem options: `{}`
- kernel: `{}`
- run root: `{}`
- artifact: `{}`
- commit: `{}`
- git worktree dirty excluding this artifact: `{}`
- substrate kind: `storage-only`
- command:

```sh
{}
```

This run primes the run-root-local empty overlay template once, then measures
`Rootfs::prepare` for N overlay clones. The base rootfs is a placeholder file
because `Rootfs::prepare` does not read or verify the base contents; callers own
base-image verification before this storage step.

## Overlay Template

- overlay size bytes: `{}`
- template logical size bytes: `{}`
- template allocated bytes after hole digging: `{}`
- clone mode: `byte-copy fallback` (`ext4` is classified non-reflink by the runtime gate)

## Observable

- samples: `{}`
- phase_3b_rootfs_prepare p50_ms: `{:.3}`
- phase_3b_rootfs_prepare p95_ms: `{:.3}`
- phase_3b_rootfs_prepare p99_ms: `{:.3}`
- phase_3b_rootfs_prepare min_ms: `{:.3}`
- phase_3b_rootfs_prepare max_ms: `{:.3}`
- reconsider dm-snapshot threshold: `p50 > 80 ms or p95 > 100 ms`
- threshold result: `{}`

## Device-Mapper Comparison

dm-snapshot was not prototyped in this run. The measured byte-copy fallback is
below the reconsider threshold, and adding a dm-snapshot prototype would touch
device-mapper setup/teardown, which the m80 audit-sweep doctrine treats as a
single-purpose kernel/device-mapper diff requiring its own real-KVM smoke if it
ever becomes justified.

- dmsetup devices before: `{}`
- dmsetup devices after: `{}`
- leaked dm devices: `{}`

## Teardown Residue

- mount entries under run root before: `{}`
- mount entries under run root after: `{}`
- leaked mounts: `{}`
- run root removed: `{}`
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
        data.command,
        OVERLAY_SIZE_BYTES,
        data.template_size,
        data.template_allocated,
        data.samples,
        data.stats.p50,
        data.stats.p95,
        data.stats.p99,
        data.stats.min,
        data.stats.max,
        if data.stats.p50 > 80.0 || data.stats.p95 > 100.0 {
            "reconsider dm-snapshot"
        } else {
            "keep byte-copy fallback"
        },
        data.dm_before
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".to_owned()),
        data.dm_after
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".to_owned()),
        data.dm_leaked,
        data.mounts_before,
        data.mounts_after,
        data.mounts_leaked,
        data.run_root_removed
    )
}

fn measurement_command(samples: usize, run_root: &Path, artifact: &Path) -> String {
    format!(
        "M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1 \\\n\
         M80_EXT4_OVERLAY_TEMPLATE_SAMPLES={samples} \\\n\
         M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT={} \\\n\
         M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT={} \\\n\
         cargo test -p m80-storage --test ext4_overlay_template_clone -- --ignored --nocapture",
        run_root.display(),
        artifact.display()
    )
}
