//! Build pipeline: download → convert → mount → install → hash → manifest.
//!
//! Each step is executed in sequence; any failure surfaces with phase context.
//! With `dry_run = true`, steps are printed to stderr and no I/O is performed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::config::{parse_size, BuildConfig};
use crate::hash::sha256_file;

/// Systemd service unit embedded at build time.
const SERVICE_UNIT: &str = include_str!("../assets/m80-guestd.service");

/// Systemd workspace mount unit embedded at build time.
const WORKSPACE_MOUNT_UNIT: &str = include_str!("../assets/workspace.mount");

/// Firecracker-CI S3 base URL.
const FC_CI_BASE: &str = "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci";

/// Canonical in-VM path for the guest daemon binary.
const GUEST_DAEMON_PATH: &str = "/usr/local/bin/m80-guestd";

/// In-VM path where the service unit is installed.
const GUEST_SERVICE_PATH: &str = "/etc/systemd/system/m80-guestd.service";

/// In-VM path where the workspace mount unit is installed.
const GUEST_MOUNT_PATH: &str = "/etc/systemd/system/workspace.mount";

/// In-VM workspace directory created as the mount point.
const GUEST_WORKSPACE_DIR: &str = "/workspace";

/// Resolved paths for a completed build.
struct BuildPaths {
    kernel: PathBuf,
    source_rootfs: PathBuf,
    output_rootfs: PathBuf,
    service_unit: PathBuf,
    workspace_mount: PathBuf,
}

/// Run the full build pipeline or print a dry-run plan.
pub fn run_build(config_path: PathBuf, dry_run: bool) -> anyhow::Result<()> {
    let cfg = BuildConfig::from_file(&config_path)?;
    let size_bytes = parse_size(&cfg.rootfs.size)
        .with_context(|| format!("parsing rootfs.size '{}'", cfg.rootfs.size))?;

    let kernel = cfg.output.dir.join("vmlinux");
    let source_rootfs = cfg.output.dir.join("source.ext4");
    let output_rootfs = cfg.output.dir.join("output.ext4");
    let service_unit_host = cfg.output.dir.join("m80-guestd.service");
    let workspace_mount_host = cfg.output.dir.join("workspace.mount");
    let manifest_path = {
        let mut p = output_rootfs.clone().into_os_string();
        p.push(".manifest.json");
        PathBuf::from(p)
    };

    let kernel_url = format!(
        "{}/{}/{}/vmlinux-5.10.225",
        FC_CI_BASE, cfg.kernel.version, cfg.kernel.arch
    );
    let rootfs_url = format!(
        "{}/{}/{}/ubuntu-22.04.squashfs",
        FC_CI_BASE, cfg.kernel.version, cfg.kernel.arch
    );

    let steps: Vec<String> = vec![
        format!(
            "1. Download kernel: curl -fsSL '{}' → {}",
            kernel_url,
            kernel.display()
        ),
        format!(
            "2. Download source rootfs squashfs: curl -fsSL '{}' → {}",
            rootfs_url,
            source_rootfs.display()
        ),
        format!(
            "3. Convert squashfs → ext4 in temp dir: unsquashfs + mkfs.ext4"
        ),
        format!(
            "4. Resize ext4 to {} bytes: truncate -s {} {}",
            size_bytes,
            size_bytes,
            output_rootfs.display()
        ),
        format!(
            "5. Loop-mount {} read-write",
            output_rootfs.display()
        ),
        format!(
            "6. Copy {} → <mount>{}",
            cfg.guestd.binary.display(),
            GUEST_DAEMON_PATH
        ),
        format!(
            "7. Write service unit → <mount>{}",
            GUEST_SERVICE_PATH
        ),
        format!(
            "8. Write workspace mount unit → <mount>{}",
            GUEST_MOUNT_PATH
        ),
        format!(
            "9. mkdir <mount>{} + enable units in multi-user.target.wants/",
            GUEST_WORKSPACE_DIR
        ),
        "10. Unmount".to_string(),
        "11. Compute sha256 of 6 artifacts".to_string(),
        format!(
            "12. Write manifest → {}",
            manifest_path.display()
        ),
    ];

    if dry_run {
        for step in &steps {
            eprintln!("{}", step);
        }
        return Ok(());
    }

    // Only create the output dir when we're actually going to write.
    std::fs::create_dir_all(&cfg.output.dir).with_context(|| {
        format!("creating output dir {}", cfg.output.dir.display())
    })?;

    let paths = BuildPaths {
        kernel: kernel.clone(),
        source_rootfs: source_rootfs.clone(),
        output_rootfs: output_rootfs.clone(),
        service_unit: service_unit_host.clone(),
        workspace_mount: workspace_mount_host.clone(),
    };

    // Step 1: download kernel.
    run_curl(&kernel_url, &kernel).context("step 1: download kernel")?;

    // Step 2: download source rootfs squashfs.
    let squashfs = cfg.output.dir.join("source.squashfs");
    run_curl(&rootfs_url, &squashfs).context("step 2: download source rootfs")?;

    // Step 3: convert squashfs → ext4.
    squashfs_to_ext4(&squashfs, &source_rootfs, &cfg.output.dir)
        .context("step 3: convert squashfs → ext4")?;

    // Step 4: copy to output + resize.
    std::fs::copy(&source_rootfs, &output_rootfs)
        .context("step 4a: copy source to output rootfs")?;
    truncate_file(&output_rootfs, size_bytes).context("step 4b: resize output rootfs")?;

    // Steps 5-9: mount + install.
    let mount_dir = tempfile::Builder::new()
        .prefix("m80-build-mnt-")
        .tempdir()
        .context("step 5: creating temp mount dir")?;
    loop_mount(&output_rootfs, mount_dir.path()).context("step 5: loop-mount output rootfs")?;

    let install_result = install_into_rootfs(mount_dir.path(), &cfg.guestd.binary);

    // Step 10: unmount before checking install result so we don't leak mounts.
    let umount_result = loop_umount(mount_dir.path()).context("step 10: umount");
    install_result.context("steps 6-9: chroot install")?;
    umount_result?;

    // Step 11: write unit files to output dir (they were embedded → written to host).
    std::fs::write(&service_unit_host, SERVICE_UNIT)
        .context("writing m80-guestd.service to output dir")?;
    std::fs::write(&workspace_mount_host, WORKSPACE_MOUNT_UNIT)
        .context("writing workspace.mount to output dir")?;

    // Step 12: hash all six artifacts.
    let kernel_sha = sha256_file(&paths.kernel).context("sha256 kernel")?;
    let source_sha = sha256_file(&paths.source_rootfs).context("sha256 source rootfs")?;
    let output_sha = sha256_file(&paths.output_rootfs).context("sha256 output rootfs")?;
    let daemon_sha = sha256_file(&cfg.guestd.binary).context("sha256 daemon binary")?;
    let service_sha = sha256_file(&paths.service_unit).context("sha256 service unit")?;
    let mount_sha = sha256_file(&paths.workspace_mount).context("sha256 workspace mount")?;

    // Step 13: emit manifest.
    let manifest = m80_image_manifest::Manifest {
        boot_target: "multi-user.target".to_string(),
        daemon_binary_path: PathBuf::from(GUEST_DAEMON_PATH),
        daemon_binary_sha256: daemon_sha,
        expected_firecracker_version: cfg.kernel.version.clone(),
        guest_port: 9001,
        kernel_image: paths.kernel.clone(),
        kernel_image_sha256: kernel_sha,
        no_egress_reason: None,
        output_rootfs_image: paths.output_rootfs.clone(),
        output_rootfs_sha256: output_sha,
        ready_marker: "GUESTD_READY".to_string(),
        schema_version: m80_image_manifest::SCHEMA_VERSION,
        service_unit_path: paths.service_unit.clone(),
        service_unit_sha256: service_sha,
        source_rootfs_image: paths.source_rootfs.clone(),
        source_rootfs_sha256: source_sha,
        workspace_mount_path: paths.workspace_mount.clone(),
        workspace_mount_sha256: mount_sha,
    };
    manifest
        .write(&manifest_path)
        .with_context(|| format!("writing manifest to {}", manifest_path.display()))?;

    println!("kernel:        {}", paths.kernel.display());
    println!("source_rootfs: {}", paths.source_rootfs.display());
    println!("output_rootfs: {}", paths.output_rootfs.display());
    println!("manifest:      {}", manifest_path.display());
    Ok(())
}

/// Run `curl -fsSL -o <dest> <url>`.
fn run_curl(url: &str, dest: &Path) -> anyhow::Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .context("spawning curl")?;
    if !status.success() {
        anyhow::bail!("curl failed (exit {:?}) downloading {}", status.code(), url);
    }
    Ok(())
}

/// Convert a squashfs image to a raw ext4 image.
///
/// Uses `unsquashfs` to extract into a temp subdir, then `mkfs.ext4` with
/// `-d` to populate a new image file from that directory tree.
fn squashfs_to_ext4(squashfs: &Path, ext4: &Path, work_dir: &Path) -> anyhow::Result<()> {
    let squash_out = work_dir.join("squashfs-root");
    // Remove any leftover extraction dir so unsquashfs -d works.
    if squash_out.exists() {
        std::fs::remove_dir_all(&squash_out).context("removing stale squashfs-root")?;
    }
    let status = Command::new("unsquashfs")
        .args(["-d"])
        .arg(&squash_out)
        .arg(squashfs)
        .status()
        .context("spawning unsquashfs")?;
    if !status.success() {
        anyhow::bail!("unsquashfs failed (exit {:?})", status.code());
    }
    let status = Command::new("mkfs.ext4")
        .args(["-d"])
        .arg(&squash_out)
        .arg(ext4)
        .status()
        .context("spawning mkfs.ext4")?;
    if !status.success() {
        anyhow::bail!("mkfs.ext4 failed (exit {:?})", status.code());
    }
    // Clean up the extracted tree.
    std::fs::remove_dir_all(&squash_out).context("removing squashfs-root after mkfs")?;
    Ok(())
}

/// Resize a file to exactly `size_bytes` using `truncate`.
fn truncate_file(path: &Path, size_bytes: u64) -> anyhow::Result<()> {
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
fn loop_mount(image: &Path, mount_dir: &Path) -> anyhow::Result<()> {
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
fn loop_umount(mount_dir: &Path) -> anyhow::Result<()> {
    let status = Command::new("umount")
        .arg(mount_dir)
        .status()
        .context("spawning umount")?;
    if !status.success() {
        anyhow::bail!("umount failed (exit {:?})", status.code());
    }
    Ok(())
}

/// Install the daemon binary and systemd units into the mounted rootfs.
///
/// Writes unit files from the embedded strings; copies the daemon binary;
/// creates the workspace directory; enables units in
/// `multi-user.target.wants/`.
fn install_into_rootfs(mount: &Path, daemon_binary: &Path) -> anyhow::Result<()> {
    // Step 6: copy daemon binary.
    let guest_bin = mount.join("usr/local/bin/m80-guestd");
    std::fs::create_dir_all(guest_bin.parent().unwrap())
        .context("creating /usr/local/bin in rootfs")?;
    std::fs::copy(daemon_binary, &guest_bin).context("copying m80-guestd into rootfs")?;
    set_executable(&guest_bin).context("chmod +x m80-guestd")?;

    // Step 7: install service unit.
    let svc_dest = mount.join("etc/systemd/system/m80-guestd.service");
    std::fs::create_dir_all(svc_dest.parent().unwrap())
        .context("creating /etc/systemd/system in rootfs")?;
    std::fs::write(&svc_dest, SERVICE_UNIT).context("writing m80-guestd.service")?;

    // Step 8: install workspace mount unit.
    let mnt_dest = mount.join("etc/systemd/system/workspace.mount");
    std::fs::write(&mnt_dest, WORKSPACE_MOUNT_UNIT).context("writing workspace.mount")?;

    // Step 9: create workspace mountpoint + enable units.
    let workspace = mount.join("workspace");
    std::fs::create_dir_all(&workspace).context("creating /workspace in rootfs")?;
    let wants = mount.join("etc/systemd/system/multi-user.target.wants");
    std::fs::create_dir_all(&wants)
        .context("creating multi-user.target.wants in rootfs")?;
    std::os::unix::fs::symlink(
        "/etc/systemd/system/m80-guestd.service",
        wants.join("m80-guestd.service"),
    )
    .context("symlinking m80-guestd.service into multi-user.target.wants")?;
    std::os::unix::fs::symlink(
        "/etc/systemd/system/workspace.mount",
        wants.join("workspace.mount"),
    )
    .context("symlinking workspace.mount into multi-user.target.wants")?;

    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .context("stat for chmod")?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).context("set_permissions 0o755")
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}
