//! `m80-image-build` — build the m80 guest image and provenance manifest.
//!
//! See `README.md` for the contract.
//! Behavior captures: bead epic `m80-sz1` (`br show m80-sz1`).
//!
//! Downloads the kernel and source rootfs from firecracker-ci S3, converts
//! the squashfs to ext4, loop-mounts it, installs m80-guestd + systemd units,
//! unmounts, hashes all six artifacts, and emits `<rootfs>.manifest.json`.
//!
//! Downloads use `curl`; mounts use system `mount`/`umount`. No new HTTP
//! client dependency.

mod config;
mod hash;
mod minimal;
mod pipeline;

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};

/// Build the m80 guest image and provenance manifest.
#[derive(Debug, Parser)]
#[command(name = "m80-image-build")]
struct Args {
    #[command(subcommand)]
    subcommand: Cmd,
}

/// Top-level subcommand.
#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run the full build pipeline using the config at the given path.
    Run {
        /// Path to the build config TOML.
        #[arg(long)]
        config: PathBuf,
        /// Print steps to stderr without performing any I/O.
        #[arg(long)]
        dry_run: bool,
    },
    /// Re-verify an already-built rootfs against its manifest.
    Verify {
        /// Path to the output ext4 rootfs image.
        #[arg(long)]
        rootfs: PathBuf,
    },
    /// Remove intermediate build artifacts (loop mount points, temp images).
    Clean {
        /// Path to the work directory to remove.
        #[arg(long)]
        workdir: PathBuf,
    },
    /// Build or manage the stripped kernel.
    Kernel {
        #[command(subcommand)]
        action: KernelAction,
    },
}

/// Kernel sub-actions.
#[derive(Debug, Subcommand)]
enum KernelAction {
    /// Build the stripped kernel via Docker.
    /// Produces `kernels/vmlinux-m80-<config-sha>.bin` under the workspace root.
    Build {
        /// Workspace root (default: current directory).
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
    },
}

fn run_verify(rootfs: PathBuf) -> anyhow::Result<()> {
    let manifest_path = pipeline::manifest_path(&rootfs);
    let manifest = m80_image_manifest::Manifest::read(&manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let root = rootfs
        .parent()
        .ok_or_else(|| anyhow::anyhow!("rootfs path has no parent: {}", rootfs.display()))?;
    manifest
        .verify(root)
        .with_context(|| format!("verifying manifest at {}", manifest_path.display()))?;
    println!("verified");
    Ok(())
}

fn run_clean(workdir: PathBuf) -> anyhow::Result<()> {
    if !workdir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&workdir)
        .with_context(|| format!("removing workdir {}", workdir.display()))
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.subcommand {
        Cmd::Run { config, dry_run } => pipeline::run_build(config, dry_run),
        Cmd::Verify { rootfs } => run_verify(rootfs),
        Cmd::Clean { workdir } => run_clean(workdir),
        Cmd::Kernel {
            action: KernelAction::Build { workspace },
        } => {
            let vmlinux = pipeline::build_stripped_kernel(&workspace)?;
            println!("vmlinux: {}", vmlinux.display());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn try_parse(args: &[&str]) -> Result<Args, clap::Error> {
        // Prepend a fake argv[0] so clap sees the full argv.
        let full: Vec<&str> = std::iter::once("m80-image-build")
            .chain(args.iter().copied())
            .collect();
        Args::try_parse_from(full)
    }

    #[test]
    fn parse_run_dry_run() {
        let args = try_parse(&["run", "--config", "/tmp/foo.toml", "--dry-run"]).unwrap();
        let Cmd::Run { config, dry_run } = args.subcommand else {
            panic!("expected Run")
        };
        assert_eq!(config, PathBuf::from("/tmp/foo.toml"));
        assert!(dry_run);
    }

    #[test]
    fn parse_verify() {
        let args = try_parse(&["verify", "--rootfs", "/tmp/r.ext4"]).unwrap();
        let Cmd::Verify { rootfs } = args.subcommand else {
            panic!("expected Verify")
        };
        assert_eq!(rootfs, PathBuf::from("/tmp/r.ext4"));
    }

    #[test]
    fn parse_missing_subcommand_fails() {
        assert!(try_parse(&[]).is_err());
    }

    #[test]
    fn parse_run_missing_config_fails() {
        assert!(try_parse(&["run"]).is_err());
    }

    #[test]
    fn parse_kernel_build_default_workspace() {
        let args = try_parse(&["kernel", "build"]).unwrap();
        let Cmd::Kernel {
            action: KernelAction::Build { workspace },
        } = args.subcommand
        else {
            panic!("expected Kernel/Build")
        };
        assert_eq!(workspace, PathBuf::from("."));
    }

    #[test]
    fn parse_kernel_missing_sub_action_fails() {
        assert!(try_parse(&["kernel"]).is_err());
    }
}
