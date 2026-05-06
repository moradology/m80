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
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::config::{parse_size, BuildConfig};
use crate::hash::sha256_file;
use crate::pipeline::{loop_mount, loop_umount, run_curl, set_executable, truncate_file};

const FC_CI_BASE: &str = "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci";
const HOST_BUSYBOX: &str = "/bin/busybox";
const PID_ONE_MOUNTPOINT_DIRS: &[&str] = &[
    "workspace",
    "proc",
    "sys",
    "dev",
    "lower",
    "upper",
    "merged",
];

/// Busybox applet symlinks installed at `/bin/<applet>`. Picked so the
/// smoke test (`/bin/echo smoke-passes`) and any common m80-guestd
/// child invocations work.
const BUSYBOX_APPLETS: &[&str] = &[
    "sh", "echo", "cat", "ls", "mkdir", "mount", "umount", "stat", "ln", "touch", "true", "false",
];

pub(crate) fn run_build_minimal(cfg: BuildConfig, dry_run: bool) -> anyhow::Result<()> {
    let size_bytes = parse_size(&cfg.rootfs.size)
        .with_context(|| format!("parsing rootfs.size '{}'", cfg.rootfs.size))?;

    let kernel = cfg.output.dir.join("vmlinux");
    let output_rootfs = cfg.output.dir.join("output.ext4");
    let daemon_binary_host = cfg.output.dir.join("m80-guestd");
    let manifest_path = {
        let mut p = output_rootfs.clone().into_os_string();
        p.push(".manifest.json");
        PathBuf::from(p)
    };

    let kernel_url = format!(
        "{}/{}/{}/vmlinux-5.10.245",
        FC_CI_BASE, cfg.kernel.version, cfg.kernel.arch
    );

    let steps: Vec<String> = vec![
        format!(
            "1. Download kernel: curl -fsSL '{}' → {}",
            kernel_url,
            kernel.display()
        ),
        format!(
            "2. Pre-allocate empty rootfs: truncate -s {} {}",
            size_bytes,
            output_rootfs.display()
        ),
        format!("3. mkfs.ext4 -F {}", output_rootfs.display()),
        format!("4. Loop-mount {}", output_rootfs.display()),
        format!(
            "5. Copy {} → <mount>/bin/busybox + symlink applets ({})",
            HOST_BUSYBOX,
            BUSYBOX_APPLETS.join(", ")
        ),
        format!(
            "6. Copy {} → <mount>/m80-guestd + symlink /init → /m80-guestd",
            cfg.guestd.binary.display()
        ),
        format!(
            "7. mkdir {} (PID-1 mount targets)",
            PID_ONE_MOUNTPOINT_DIRS
                .iter()
                .map(|d| format!("/{d}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "8. Unmount".to_string(),
        "9. Compute sha256 of 3 artifacts (kernel, output_rootfs, daemon_binary)".to_string(),
        format!("10. Write manifest → {}", manifest_path.display()),
    ];

    if dry_run {
        for step in &steps {
            eprintln!("{}", step);
        }
        return Ok(());
    }

    if !Path::new(HOST_BUSYBOX).exists() {
        anyhow::bail!(
            "host {} not found — install busybox-static (Debian/Ubuntu: \
             `apt install busybox-static`) or place a static busybox at {}",
            HOST_BUSYBOX,
            HOST_BUSYBOX
        );
    }

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
    loop_mount(&output_rootfs, mount_dir.path()).context("step 4: loop-mount output rootfs")?;

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
    let manifest = m80_image_manifest::Manifest {
        boot_target: None,
        daemon_binary_path: daemon_binary_host.clone(),
        daemon_binary_sha256: daemon_sha,
        expected_firecracker_version: cfg.kernel.version.clone(),
        guest_port: m80_proto::GUEST_PORT_DEFAULT,
        image_kind: m80_image_manifest::ImageKind::Minimal,
        kernel_image: kernel.clone(),
        kernel_image_sha256: kernel_sha,
        kernel_kind: m80_image_manifest::KernelKind::Stock,
        no_egress_reason: Some(m80_image_manifest::DEFAULT_NO_EGRESS_REASON.to_owned()),
        output_rootfs_image: output_rootfs.clone(),
        output_rootfs_sha256: output_sha,
        ready_marker: m80_proto::READY_MARKER_DEFAULT.to_string(),
        schema_version: m80_image_manifest::SCHEMA_VERSION,
        service_unit_path: None,
        service_unit_sha256: None,
        source_rootfs_image: None,
        source_rootfs_sha256: None,
        workspace_mount_path: None,
        workspace_mount_sha256: None,
    };
    manifest
        .write(&manifest_path)
        .with_context(|| format!("writing manifest to {}", manifest_path.display()))?;

    println!("kernel:        {}", kernel.display());
    println!("output_rootfs: {}", output_rootfs.display());
    println!("manifest:      {}", manifest_path.display());
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

    // Install m80-guestd at /m80-guestd.
    let guestd_dest = mount.join("m80-guestd");
    std::fs::copy(daemon_binary, &guestd_dest).context("copying m80-guestd into rootfs")?;
    set_executable(&guestd_dest).context("chmod +x m80-guestd")?;

    // /init → /m80-guestd (kernel's init= target).
    std::os::unix::fs::symlink("/m80-guestd", mount.join("init"))
        .context("symlinking /init → /m80-guestd")?;

    // Mountpoint dirs for the PID-1 setup in m80-guestd::pid_one.
    for d in PID_ONE_MOUNTPOINT_DIRS {
        std::fs::create_dir_all(mount.join(d))
            .with_context(|| format!("creating /{d} in rootfs"))?;
    }
    std::fs::create_dir_all(mount.join("tmp")).context("creating /tmp in rootfs")?;
    std::fs::set_permissions(mount.join("tmp"), std::fs::Permissions::from_mode(0o1777))
        .context("chmod 1777 /tmp")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BUSYBOX_APPLETS, PID_ONE_MOUNTPOINT_DIRS};

    #[test]
    fn pid_one_mountpoints_include_overlay_pivot_targets() {
        assert_eq!(
            PID_ONE_MOUNTPOINT_DIRS,
            &[
                "workspace",
                "proc",
                "sys",
                "dev",
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
