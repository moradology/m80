//! Build pipeline: download → convert → mount → install → hash → manifest.
//!
//! Each step is executed in sequence; any failure surfaces with phase context.
//! With `dry_run = true`, steps are printed to stderr and no I/O is performed.
//!
//! The `build_stripped_kernel` function handles the `kernel build`
//! path: Docker-builds the kernel, runs the container, and copies the output
//! vmlinux to `kernels/vmlinux-m80-<config-sha>.bin`.

use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::Context;
use m80_image_manifest::{BuildReceipt, BuildReceiptArtifact, BuildReceiptArtifactKind};
use nix::mount::MsFlags;
use nix::sched::CloneFlags;

use crate::config::{parse_size, BuildConfig};
use crate::hash::sha256_file;
use crate::minimal;

// Filenames drift per Firecracker CI artifact track: v1.15 currently ships
// these names. TODO(v0.2): probe the bucket index instead of hardcoding.
pub(crate) const KERNEL_FILENAME: &str = "vmlinux-5.10.245";
pub(crate) const UBUNTU_SQUASHFS: &str = "ubuntu-24.04.squashfs";

/// Return the manifest path for a rootfs image: `<rootfs>.manifest.json`.
pub(crate) fn manifest_path(rootfs: &Path) -> PathBuf {
    let mut name = rootfs.file_name().unwrap_or_default().to_owned();
    name.push(".manifest.json");
    rootfs.with_file_name(name)
}

/// Return the build receipt path for a rootfs image:
/// `<rootfs>.build-receipt.json`.
pub(crate) fn build_receipt_path(rootfs: &Path) -> PathBuf {
    let mut name = rootfs.file_name().unwrap_or_default().to_owned();
    name.push(".build-receipt.json");
    rootfs.with_file_name(name)
}

/// Construct a [`m80_image_manifest::Manifest`] from the artifacts common to
/// both build paths. The `image_kind`, `source_rootfs_image`, and
/// `source_rootfs_sha256` fields differ per path and are supplied by the
/// caller.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_manifest(
    daemon_binary_host: std::path::PathBuf,
    daemon_sha: String,
    kernel_version: String,
    image_kind: m80_image_manifest::ImageKind,
    kernel_path: std::path::PathBuf,
    kernel_sha: String,
    output_rootfs: std::path::PathBuf,
    output_sha: String,
    rootfs_format: m80_image_manifest::RootfsFormat,
    source_rootfs: Option<std::path::PathBuf>,
    source_sha: Option<String>,
) -> m80_image_manifest::Manifest {
    m80_image_manifest::Manifest::new(
        daemon_binary_host,
        daemon_sha,
        kernel_version,
        m80_proto::GUEST_PORT_DEFAULT,
        image_kind,
        kernel_path,
        kernel_sha,
        m80_image_manifest::KernelKind::Stock,
        Some(m80_image_manifest::DEFAULT_NO_EGRESS_REASON.to_owned()),
        output_rootfs,
        output_sha,
        m80_proto::READY_MARKER_DEFAULT.to_string(),
        rootfs_format,
        source_rootfs,
        source_sha,
    )
}
pub(crate) const FC_CI_BASE: &str = "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci";
const GUEST_DAEMON_PATH: &str = "/m80-guestd";
pub(crate) const PID_ONE_MOUNTPOINT_DIRS: &[&str] = &[
    "workspace",
    "proc",
    "sys",
    "dev",
    "etc",
    "lower",
    "upper",
    "merged",
];

/// Run the full build pipeline or print a dry-run plan. Dispatches on
/// `cfg.rootfs.kind`: `"ubuntu"` (default) builds from the firecracker-ci
/// squashfs; `"minimal"` builds an empty ext4 with busybox + static
/// guestd as PID 1; `"minimal-erofs"` builds the same userland as a
/// compressed read-only erofs image.
pub(crate) fn run_build(config_path: &Path, dry_run: bool) -> anyhow::Result<()> {
    let cfg = BuildConfig::from_file(config_path)?;
    match cfg.rootfs.kind.as_deref() {
        Some("minimal") => minimal::run_build_minimal(cfg, dry_run),
        Some("minimal-erofs") => minimal::run_build_minimal_erofs(cfg, dry_run),
        Some("ubuntu") | None => run_build_ubuntu(cfg, dry_run),
        Some(other) => {
            anyhow::bail!(
                "unknown rootfs.kind '{other}': expected \"ubuntu\", \"minimal\", or \"minimal-erofs\""
            )
        }
    }
}

fn run_build_ubuntu(cfg: BuildConfig, dry_run: bool) -> anyhow::Result<()> {
    let size_bytes = parse_size(&cfg.rootfs.size)
        .with_context(|| format!("parsing rootfs.size '{}'", cfg.rootfs.size))?;

    let kernel = cfg.output.dir.join("vmlinux");
    let source_rootfs = cfg.output.dir.join("source.ext4");
    let output_rootfs = cfg.output.dir.join("output.ext4");
    // Host audit copy of the daemon binary so `Manifest::verify` (which
    // runs at preflight time and does not loop-mount the rootfs) has a
    // host-readable artifact to sha256. The in-VM destination is a build
    // constant (`GUEST_DAEMON_PATH`) and not the manifest's concern.
    let daemon_binary_host = cfg.output.dir.join("m80-guestd");
    let manifest_path = manifest_path(&output_rootfs);
    let build_receipt_path = build_receipt_path(&output_rootfs);

    let kernel_url = format!(
        "{}/{}/{}/{}",
        FC_CI_BASE, cfg.kernel.artifact_track, cfg.kernel.arch, KERNEL_FILENAME
    );
    let rootfs_url = format!(
        "{}/{}/{}/{}",
        FC_CI_BASE, cfg.kernel.artifact_track, cfg.kernel.arch, UBUNTU_SQUASHFS
    );

    if dry_run {
        eprintln!(
            "1. Download kernel: curl -fsSL '{}' → {}",
            kernel_url,
            kernel.display()
        );
        eprintln!(
            "2. Download source rootfs squashfs: curl -fsSL '{}' → {}",
            rootfs_url,
            source_rootfs.display()
        );
        eprintln!(
            "3. Convert squashfs → ext4 in temp dir: unsquashfs -no-xattrs + SUID/SGID strip + mkfs.ext4"
        );
        eprintln!(
            "4. Resize ext4 to {} bytes: truncate -s {} {}",
            size_bytes,
            size_bytes,
            output_rootfs.display()
        );
        eprintln!("5. Loop-mount {} read-write", output_rootfs.display());
        eprintln!(
            "6. Copy {} → <mount>{}",
            cfg.guestd.binary.display(),
            GUEST_DAEMON_PATH
        );
        eprintln!(
            "7. mkdir {} (PID-1 mount targets) + symlink <mount>/init → /m80-guestd",
            PID_ONE_MOUNTPOINT_DIRS
                .iter()
                .map(|d| format!("/{d}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        eprintln!("8. Unmount");
        eprintln!("9. Compute sha256 of 4 artifacts");
        eprintln!("10. Write manifest → {}", manifest_path.display());
        eprintln!("11. Write build receipt → {}", build_receipt_path.display());
        return Ok(());
    }

    // Only create the output dir when we're actually going to write.
    std::fs::create_dir_all(&cfg.output.dir)
        .with_context(|| format!("creating output dir {}", cfg.output.dir.display()))?;

    // Step 1: download kernel.
    run_curl(&kernel_url, &kernel).context("step 1: download kernel")?;

    // Step 2: download source rootfs squashfs.
    let squashfs = cfg.output.dir.join("source.squashfs");
    run_curl(&rootfs_url, &squashfs).context("step 2: download source rootfs")?;

    // Step 3: convert squashfs → ext4. Pre-allocate the target to the
    // configured rootfs size so mkfs.ext4 has somewhere to write — without
    // a pre-sized file (or an explicit size argument) mkfs.ext4 errors out.
    squashfs_to_ext4(&squashfs, &source_rootfs, &cfg.output.dir, size_bytes)
        .context("step 3: convert squashfs → ext4")?;

    // Step 4: copy to output + resize.
    std::fs::copy(&source_rootfs, &output_rootfs)
        .context("step 4a: copy source to output rootfs")?;
    truncate_file(&output_rootfs, size_bytes).context("step 4b: resize output rootfs")?;

    // Steps 5-7: mount + install.
    let mount_dir = tempfile::Builder::new()
        .prefix("m80-build-mnt-")
        .tempdir()
        .context("step 5: creating temp mount dir")?;
    enter_private_mount_namespace().context("step 5: isolate loop mount namespace")?;
    loop_mount(&output_rootfs, mount_dir.path()).context("step 5: loop-mount output rootfs")?;
    maybe_sleep_after_loop_mount(mount_dir.path()).context("test hook after loop mount")?;

    let install_result = install_pid_one_artifacts(mount_dir.path(), &cfg.guestd.binary);

    // Step 8: unmount before checking install result so we don't leak mounts.
    let umount_result = loop_umount(mount_dir.path()).context("step 8: umount");
    install_result.context("steps 6-7: chroot install")?;
    umount_result?;

    // Step 9: host audit copy of the daemon binary that was installed inside the
    // rootfs at GUEST_DAEMON_PATH.
    std::fs::copy(&cfg.guestd.binary, &daemon_binary_host)
        .context("copying m80-guestd to output dir")?;

    // Step 9: hash all four artifacts.
    let kernel_sha = sha256_file(&kernel).context("sha256 kernel")?;
    let source_sha = sha256_file(&source_rootfs).context("sha256 source rootfs")?;
    let output_sha = sha256_file(&output_rootfs).context("sha256 output rootfs")?;
    let daemon_sha = sha256_file(&cfg.guestd.binary).context("sha256 daemon binary")?;

    // Step 10: emit manifest.
    let manifest = build_manifest(
        daemon_binary_host,
        daemon_sha,
        cfg.kernel.version,
        m80_image_manifest::ImageKind::Ubuntu,
        kernel.clone(),
        kernel_sha,
        output_rootfs.clone(),
        output_sha,
        m80_image_manifest::RootfsFormat::Ext4,
        Some(source_rootfs.clone()),
        Some(source_sha),
    );
    manifest
        .write(&manifest_path)
        .with_context(|| format!("writing manifest to {}", manifest_path.display()))?;
    emit_build_receipt(
        &build_receipt_path,
        &manifest_path,
        vec![
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::KernelImage,
                path: kernel.clone(),
                sha256: manifest.kernel_image_sha256.clone(),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::SourceRootfsImage,
                path: source_rootfs.clone(),
                sha256: manifest
                    .source_rootfs_sha256
                    .clone()
                    .expect("Ubuntu source sha"),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::OutputRootfsImage,
                path: output_rootfs.clone(),
                sha256: manifest.output_rootfs_sha256.clone(),
            },
            BuildReceiptArtifact {
                kind: BuildReceiptArtifactKind::DaemonBinaryPath,
                path: manifest.daemon_binary_path.clone(),
                sha256: manifest.daemon_binary_sha256.clone(),
            },
        ],
    )?;

    println!("kernel:        {}", kernel.display());
    println!("source_rootfs: {}", source_rootfs.display());
    println!("output_rootfs: {}", output_rootfs.display());
    println!("manifest:      {}", manifest_path.display());
    println!("receipt:       {}", build_receipt_path.display());
    Ok(())
}

pub(crate) fn emit_build_receipt(
    receipt_path: &Path,
    manifest_path: &Path,
    artifacts: Vec<BuildReceiptArtifact>,
) -> anyhow::Result<()> {
    let manifest_sha = sha256_file(manifest_path).context("sha256 manifest")?;
    let receipt = BuildReceipt::new(manifest_path.to_path_buf(), manifest_sha, artifacts);
    receipt
        .write(receipt_path)
        .with_context(|| format!("writing build receipt to {}", receipt_path.display()))
}

/// Run `curl -fsSL -o <dest> <url>`.
pub(crate) fn run_curl(url: &str, dest: &Path) -> anyhow::Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .context("spawning curl")?;
    if !status.success() {
        anyhow::bail!(
            "curl failed (exit {}) downloading {}",
            format_exit(status),
            url
        );
    }
    Ok(())
}

/// Convert a squashfs image to a raw ext4 image. The target file must be
/// pre-sized with `truncate` before `mkfs.ext4 -d` (and we pass `-F` to
/// accept the existing file) — without that, mkfs errors with "file does
/// not exist and no size was specified".
///
/// 3-phase: clean stale dir → unsquashfs + mkfs.ext4 → clean temp dir.
fn squashfs_to_ext4(
    squashfs: &Path,
    ext4: &Path,
    work_dir: &Path,
    size_bytes: u64,
) -> anyhow::Result<()> {
    let squash_out = work_dir.join("squashfs-root");
    if squash_out.exists() {
        anyhow::bail!(
            "squashfs-root already exists at {} — remove it before building",
            squash_out.display()
        );
    }
    let status = unsquashfs_command(squashfs, &squash_out)
        .status()
        .context("spawning unsquashfs")?;
    if !status.success() {
        anyhow::bail!("unsquashfs failed (exit {})", format_exit(status));
    }
    strip_suid_sgid_bits(&squash_out).context("stripping SUID/SGID mode bits")?;
    truncate_file(ext4, size_bytes).context("pre-sizing source ext4 image")?;
    let status = Command::new("mkfs.ext4")
        .args(["-F", "-d"])
        .arg(&squash_out)
        .arg(ext4)
        .status()
        .context("spawning mkfs.ext4")?;
    if !status.success() {
        anyhow::bail!("mkfs.ext4 failed (exit {})", format_exit(status));
    }
    std::fs::remove_dir_all(&squash_out).context("removing squashfs-root after mkfs")?;
    Ok(())
}

fn unsquashfs_command(squashfs: &Path, squash_out: &Path) -> Command {
    let mut command = Command::new("unsquashfs");
    command
        .arg("-no-xattrs")
        .arg("-d")
        .arg(squash_out)
        .arg(squashfs);
    command
}

fn strip_suid_sgid_bits(root: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata =
        std::fs::symlink_metadata(root).with_context(|| format!("stat {}", root.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    let mode = metadata.permissions().mode();
    if mode & 0o6000 != 0 {
        let mut perms = metadata.permissions();
        perms.set_mode(mode & !0o6000);
        std::fs::set_permissions(root, perms)
            .with_context(|| format!("chmod a-s {}", root.display()))?;
    }

    if metadata.is_dir() {
        for entry in
            std::fs::read_dir(root).with_context(|| format!("read_dir {}", root.display()))?
        {
            let entry =
                entry.with_context(|| format!("read_dir entry under {}", root.display()))?;
            strip_suid_sgid_bits(&entry.path())?;
        }
    }

    Ok(())
}

/// Resize a file to exactly `size_bytes` using `truncate`.
pub(crate) fn truncate_file(path: &Path, size_bytes: u64) -> anyhow::Result<()> {
    let status = Command::new("truncate")
        .args(["-s", &size_bytes.to_string()])
        .arg(path)
        .status()
        .context("spawning truncate")?;
    if !status.success() {
        anyhow::bail!("truncate failed (exit {:?})", status.code());
    }
    Ok(())
}

/// Loop-mount `image` at `mount_dir` read-write.
pub(crate) fn loop_mount(image: &Path, mount_dir: &Path) -> anyhow::Result<()> {
    let status = Command::new("mount")
        .args(["-o", "loop,rw"])
        .arg(image)
        .arg(mount_dir)
        .status()
        .context("spawning mount")?;
    if !status.success() {
        anyhow::bail!("mount failed (exit {:?})", status.code());
    }
    Ok(())
}

/// Unmount `mount_dir`.
pub(crate) fn loop_umount(mount_dir: &Path) -> anyhow::Result<()> {
    let status = Command::new("umount")
        .arg(mount_dir)
        .status()
        .context("spawning umount")?;
    if !status.success() {
        anyhow::bail!("umount failed (exit {:?})", status.code());
    }
    Ok(())
}

/// Enter a private mount namespace before creating loop mounts.
///
/// If the builder is SIGKILLed while the rootfs is mounted, the namespace dies
/// with the process and the loop mount is not propagated into the host
/// namespace.
pub(crate) fn enter_private_mount_namespace() -> anyhow::Result<()> {
    nix::sched::unshare(CloneFlags::CLONE_NEWNS).context("unshare(CLONE_NEWNS)")?;
    nix::mount::mount::<str, str, str, str>(
        None,
        "/",
        None,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None,
    )
    .context("mount / MS_REC|MS_PRIVATE")
}

#[cfg(debug_assertions)]
pub(crate) fn maybe_sleep_after_loop_mount(mount_dir: &Path) -> anyhow::Result<()> {
    let Some(ready_path) = std::env::var_os("M80_TEST_SLEEP_AFTER_IMAGE_BUILD_LOOP_MOUNT") else {
        return Ok(());
    };

    std::fs::write(&ready_path, format!("{}\n", mount_dir.display())).with_context(|| {
        format!(
            "writing test ready marker {}",
            PathBuf::from(&ready_path).display()
        )
    })?;

    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn maybe_sleep_after_loop_mount(_mount_dir: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Install the daemon binary and PID-1 mountpoint contract into the mounted rootfs.
///
/// Shared by both the ubuntu and minimal build paths. Copies `daemon_binary`
/// to `<mount>/m80-guestd`, sets it executable, symlinks `/init → /m80-guestd`,
/// and creates the [`PID_ONE_MOUNTPOINT_DIRS`] inside the mount.
pub(crate) fn install_pid_one_artifacts(mount: &Path, daemon_binary: &Path) -> anyhow::Result<()> {
    let guest_bin = mount.join("m80-guestd");
    std::fs::copy(daemon_binary, &guest_bin).context("copying m80-guestd into rootfs")?;
    set_executable(&guest_bin).context("chmod +x m80-guestd")?;

    std::os::unix::fs::symlink("/m80-guestd", mount.join("init"))
        .context("symlinking /init → /m80-guestd")?;

    for d in PID_ONE_MOUNTPOINT_DIRS {
        std::fs::create_dir_all(mount.join(d))
            .with_context(|| format!("creating /{d} in rootfs"))?;
    }

    Ok(())
}

/// Build the stripped kernel via Docker and return the path of the output
/// vmlinux artifact.
///
/// Steps:
/// 1. `docker build` the `kernel-builder/` Dockerfile into image `m80-kernel-builder`.
/// 2. `docker run` with a bind-mount on the `kernels/` output directory.
/// 3. Parse the container's stdout for `output: /out/vmlinux-m80-<sha>.bin`
///    and return the host-side path.
///
/// The container's `build.sh` runs `make olddefconfig` first and hashes the
/// post-resolution `.config` — that sha is embedded in the output filename.
///
/// Requires Docker to be available on the host. If Docker is absent, the
/// command fails with the underlying I/O error.
pub(crate) fn build_stripped_kernel(workspace_root: &Path) -> anyhow::Result<PathBuf> {
    let builder_dir = workspace_root
        .join("crates")
        .join("m80-image-build")
        .join("kernel-builder");
    let kernels_dir = workspace_root
        .join("crates")
        .join("m80-image-build")
        .join("kernels");

    std::fs::create_dir_all(&kernels_dir)
        .with_context(|| format!("creating kernels dir {}", kernels_dir.display()))?;

    // Step 1: docker build.
    let status = Command::new("docker")
        .args(["build", "-t", "m80-kernel-builder"])
        .arg(&builder_dir)
        .status()
        .context("spawning docker build")?;
    if !status.success() {
        anyhow::bail!("docker build failed (exit {:?})", status.code());
    }

    // Step 2: docker run with kernels/ bind-mounted at /out.
    let mount_arg = format!("{}:/out", kernels_dir.display());
    let output = Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(&mount_arg)
        .arg("m80-kernel-builder")
        .output()
        .context("spawning docker run")?;
    if !output.status.success() {
        let detail = if output.stderr.is_empty() {
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else {
            String::from_utf8_lossy(&output.stderr).into_owned()
        };
        anyhow::bail!(
            "docker run failed (exit {}): {}",
            format_exit(output.status),
            detail
        );
    }

    // Step 3: parse container stdout for the output path.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let vmlinux_name = stdout
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("output: /out/")?;
            Some(rest.trim().to_owned())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "docker run did not print 'output: /out/<name>' line; stdout:\n{}",
                stdout
            )
        })?;

    let host_path = kernels_dir.join(&vmlinux_name);
    if !host_path.exists() {
        anyhow::bail!(
            "kernel build reported output '{}' but file not found at {}",
            vmlinux_name,
            host_path.display()
        );
    }
    Ok(host_path)
}

/// Format a process exit status for display in error messages.
///
/// Returns the numeric exit code as a string, or `"signal: <N>"` when the
/// process was terminated by a signal and `ExitStatus::code()` is `None`.
fn format_exit(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        code.to_string()
    } else if let Some(sig) = status.signal() {
        format!("signal: {sig}")
    } else {
        "unknown".to_owned()
    }
}

#[cfg(unix)]
pub(crate) fn set_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .context("stat for chmod")?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).context("set_permissions 0o755")
}

#[cfg(not(unix))]
pub(crate) fn set_executable(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
