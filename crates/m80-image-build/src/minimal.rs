//! Minimal-image build path: empty ext4 + busybox + static m80-guestd.
//!
//! No systemd, no apt, no upstream squashfs. m80-guestd is the kernel's
//! `init=` target; busybox supplies the standard userland (sh, echo, cat,
//! ls, mount, umount, etc.).
//!
//! Caller responsibilities (NOT enforced here):
//! 1. `cfg.guestd.binary` must point at a statically-linked m80-guestd
//!    (e.g., built with `--target x86_64-unknown-linux-musl`). A
//!    glibc-linked binary will fail at runtime under the busybox-only
//!    rootfs.
//! 2. The host distro must have `/bin/busybox` available — install
//!    `busybox-static` on Debian/Ubuntu.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::Context;

use crate::config::{parse_size, BuildConfig};
use crate::hash::sha256_file;
use crate::pipeline::{
    build_manifest, enter_private_mount_namespace, install_pid_one_artifacts, loop_mount,
    loop_umount, manifest_path, maybe_sleep_after_loop_mount, run_curl, set_executable,
    truncate_file, FC_CI_BASE, KERNEL_FILENAME, PID_ONE_MOUNTPOINT_DIRS,
};

const HOST_BUSYBOX: &str = "/bin/busybox";

/// Busybox applet symlinks installed at `/bin/<applet>`. Picked so the
/// smoke test (`/bin/echo smoke-passes`) and any common m80-guestd
/// child invocations work.
const BUSYBOX_APPLETS: &[&str] = &[
    "sh", "echo", "cat", "ls", "mkdir", "mount", "umount", "stat", "ln", "touch", "true", "false",
];

pub(crate) fn run_build_minimal_erofs(cfg: BuildConfig, dry_run: bool) -> anyhow::Result<()> {
    let _size_bytes = parse_size(&cfg.rootfs.size)
        .with_context(|| format!("parsing rootfs.size '{}'", cfg.rootfs.size))?;
    let kernel = cfg.output.dir.join("vmlinux");
    let output_rootfs = cfg.output.dir.join("output.erofs");
    let daemon_binary_host = cfg.output.dir.join("m80-guestd");
    let manifest_path = manifest_path(&output_rootfs);

    let kernel_url = format!(
        "{}/{}/{}/{}",
        FC_CI_BASE, cfg.kernel.artifact_track, cfg.kernel.arch, KERNEL_FILENAME
    );

    if dry_run {
        eprintln!(
            "1. Download kernel: curl -fsSL '{}' → {}",
            kernel_url,
            kernel.display()
        );
        eprintln!("2. Create temporary minimal rootfs tree");
        eprintln!(
            "3. Copy {} → <tree>/bin/busybox + symlink applets ({})",
            HOST_BUSYBOX,
            BUSYBOX_APPLETS.join(", ")
        );
        eprintln!(
            "4. Copy {} → <tree>/m80-guestd + symlink /init → /m80-guestd",
            cfg.guestd.binary.display()
        );
        eprintln!(
            "5. mkdir {} (PID-1 mount targets)",
            PID_ONE_MOUNTPOINT_DIRS
                .iter()
                .map(|d| format!("/{d}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        eprintln!(
            "6. Build read-only rootfs: mkfs.erofs -zlz4hc {} <tree>",
            output_rootfs.display()
        );
        eprintln!("7. Compute sha256 of 3 artifacts (kernel, output_rootfs, daemon_binary)");
        eprintln!("8. Write manifest → {}", manifest_path.display());
        return Ok(());
    }

    require_host_busybox()?;

    std::fs::create_dir_all(&cfg.output.dir)
        .with_context(|| format!("creating output dir {}", cfg.output.dir.display()))?;

    run_curl(&kernel_url, &kernel).context("step 1: download kernel")?;

    let tree = tempfile::Builder::new()
        .prefix("m80-erofs-tree-")
        .tempdir()
        .context("step 2: creating temporary rootfs tree")?;
    install_minimal(tree.path(), &cfg.guestd.binary).context("steps 3-5: minimal install")?;

    run_mkfs_erofs(&output_rootfs, tree.path()).context("step 6: mkfs.erofs")?;

    std::fs::copy(&cfg.guestd.binary, &daemon_binary_host)
        .context("copying m80-guestd to output dir")?;

    let kernel_sha = sha256_file(&kernel).context("sha256 kernel")?;
    let output_sha = sha256_file(&output_rootfs).context("sha256 output rootfs")?;
    let daemon_sha = sha256_file(&cfg.guestd.binary).context("sha256 daemon binary")?;

    let manifest = build_manifest(
        daemon_binary_host,
        daemon_sha,
        cfg.kernel.version,
        m80_image_manifest::ImageKind::Minimal,
        kernel.clone(),
        kernel_sha,
        output_rootfs.clone(),
        output_sha,
        m80_image_manifest::RootfsFormat::Erofs,
        None,
        None,
    );
    manifest
        .write(&manifest_path)
        .with_context(|| format!("writing manifest to {}", manifest_path.display()))?;

    println!("kernel:        {}", kernel.display());
    println!("output_rootfs: {}", output_rootfs.display());
    println!("manifest:      {}", manifest_path.display());
    Ok(())
}

pub(crate) fn run_build_minimal(cfg: BuildConfig, dry_run: bool) -> anyhow::Result<()> {
    let size_bytes = parse_size(&cfg.rootfs.size)
        .with_context(|| format!("parsing rootfs.size '{}'", cfg.rootfs.size))?;

    let kernel = cfg.output.dir.join("vmlinux");
    let output_rootfs = cfg.output.dir.join("output.ext4");
    let daemon_binary_host = cfg.output.dir.join("m80-guestd");
    let manifest_path = manifest_path(&output_rootfs);

    let kernel_url = format!(
        "{}/{}/{}/{}",
        FC_CI_BASE, cfg.kernel.artifact_track, cfg.kernel.arch, KERNEL_FILENAME
    );

    if dry_run {
        eprintln!(
            "1. Download kernel: curl -fsSL '{}' → {}",
            kernel_url,
            kernel.display()
        );
        eprintln!(
            "2. Pre-allocate empty rootfs: truncate -s {} {}",
            size_bytes,
            output_rootfs.display()
        );
        eprintln!("3. mkfs.ext4 -F {}", output_rootfs.display());
        eprintln!("4. Loop-mount {}", output_rootfs.display());
        eprintln!(
            "5. Copy {} → <mount>/bin/busybox + symlink applets ({})",
            HOST_BUSYBOX,
            BUSYBOX_APPLETS.join(", ")
        );
        eprintln!(
            "6. Copy {} → <mount>/m80-guestd + symlink /init → /m80-guestd",
            cfg.guestd.binary.display()
        );
        eprintln!(
            "7. mkdir {} (PID-1 mount targets)",
            PID_ONE_MOUNTPOINT_DIRS
                .iter()
                .map(|d| format!("/{d}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        eprintln!("8. Unmount");
        eprintln!("9. Compute sha256 of 3 artifacts (kernel, output_rootfs, daemon_binary)");
        eprintln!("10. Write manifest → {}", manifest_path.display());
        return Ok(());
    }

    require_host_busybox()?;

    std::fs::create_dir_all(&cfg.output.dir)
        .with_context(|| format!("creating output dir {}", cfg.output.dir.display()))?;

    // Step 1: download kernel.
    run_curl(&kernel_url, &kernel).context("step 1: download kernel")?;

    // Step 2-3: pre-allocate + mkfs.ext4 from scratch.
    truncate_file(&output_rootfs, size_bytes).context("step 2: pre-allocate output rootfs")?;
    let status = Command::new("mkfs.ext4")
        .arg("-F")
        .arg(&output_rootfs)
        .status()
        .context("spawning mkfs.ext4")?;
    if !status.success() {
        anyhow::bail!("mkfs.ext4 failed (exit {:?})", status.code());
    }

    // Steps 4-7: mount + install.
    let mount_dir = tempfile::Builder::new()
        .prefix("m80-build-mnt-")
        .tempdir()
        .context("step 4: creating temp mount dir")?;
    enter_private_mount_namespace().context("step 4: isolate loop mount namespace")?;
    loop_mount(&output_rootfs, mount_dir.path()).context("step 4: loop-mount output rootfs")?;
    maybe_sleep_after_loop_mount(mount_dir.path()).context("test hook after loop mount")?;

    let install_result = install_minimal(mount_dir.path(), &cfg.guestd.binary);

    // Step 8: unmount before checking install result so we don't leak mounts.
    let umount_result = loop_umount(mount_dir.path()).context("step 8: umount");
    install_result.context("steps 5-7: minimal install")?;
    umount_result?;

    // Host audit copy of the daemon binary.
    std::fs::copy(&cfg.guestd.binary, &daemon_binary_host)
        .context("copying m80-guestd to output dir")?;

    // Step 9: hash the three artifacts that exist for Minimal kind.
    let kernel_sha = sha256_file(&kernel).context("sha256 kernel")?;
    let output_sha = sha256_file(&output_rootfs).context("sha256 output rootfs")?;
    let daemon_sha = sha256_file(&cfg.guestd.binary).context("sha256 daemon binary")?;

    // Step 10: emit manifest.
    let manifest = build_manifest(
        daemon_binary_host,
        daemon_sha,
        cfg.kernel.version,
        m80_image_manifest::ImageKind::Minimal,
        kernel.clone(),
        kernel_sha,
        output_rootfs.clone(),
        output_sha,
        m80_image_manifest::RootfsFormat::Ext4,
        None,
        None,
    );
    manifest
        .write(&manifest_path)
        .with_context(|| format!("writing manifest to {}", manifest_path.display()))?;

    println!("kernel:        {}", kernel.display());
    println!("output_rootfs: {}", output_rootfs.display());
    println!("manifest:      {}", manifest_path.display());
    Ok(())
}

fn require_host_busybox() -> anyhow::Result<()> {
    if Path::new(HOST_BUSYBOX).exists() {
        return Ok(());
    }
    anyhow::bail!(
        "host {} not found — install busybox-static (Debian/Ubuntu: \
         `apt install busybox-static`) or place a static busybox at {}",
        HOST_BUSYBOX,
        HOST_BUSYBOX
    );
}

fn run_mkfs_erofs(output_rootfs: &Path, source_tree: &Path) -> anyhow::Result<()> {
    let status = Command::new("mkfs.erofs")
        .arg("-zlz4hc")
        .arg(output_rootfs)
        .arg(source_tree)
        .status()
        .context("spawning mkfs.erofs")?;
    if !status.success() {
        anyhow::bail!("mkfs.erofs failed (exit {:?})", status.code());
    }
    Ok(())
}

fn install_minimal(mount: &Path, daemon_binary: &Path) -> anyhow::Result<()> {
    // Copy busybox.
    let bin_dir = mount.join("bin");
    std::fs::create_dir_all(&bin_dir).context("creating /bin in rootfs")?;
    let busybox_dest = bin_dir.join("busybox");
    std::fs::copy(HOST_BUSYBOX, &busybox_dest).context("copying /bin/busybox into rootfs")?;
    set_executable(&busybox_dest).context("chmod +x busybox")?;

    // Symlink common applets so /bin/echo, /bin/sh, etc. work.
    for applet in BUSYBOX_APPLETS {
        let link = bin_dir.join(applet);
        std::os::unix::fs::symlink("busybox", &link)
            .with_context(|| format!("symlinking /bin/{applet} → busybox"))?;
    }

    // Install m80-guestd, /init symlink, and PID-1 mountpoint dirs.
    install_pid_one_artifacts(mount, daemon_binary)?;

    std::fs::create_dir_all(mount.join("tmp")).context("creating /tmp in rootfs")?;
    std::fs::set_permissions(mount.join("tmp"), std::fs::Permissions::from_mode(0o1777))
        .context("chmod 1777 /tmp")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::BUSYBOX_APPLETS;
    use crate::pipeline::PID_ONE_MOUNTPOINT_DIRS;

    #[test]
    fn pid_one_mountpoints_include_overlay_pivot_targets() {
        assert_eq!(
            PID_ONE_MOUNTPOINT_DIRS,
            &[
                "workspace",
                "proc",
                "sys",
                "dev",
                "etc",
                "lower",
                "upper",
                "merged"
            ]
        );
    }

    #[test]
    fn busybox_applets_include_persistent_exec_test_tools() {
        assert!(BUSYBOX_APPLETS.contains(&"sh"));
        assert!(BUSYBOX_APPLETS.contains(&"touch"));
        assert!(BUSYBOX_APPLETS.contains(&"cat"));
    }
}
