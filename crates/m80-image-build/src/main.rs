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

#![deny(missing_docs)]

mod config;
mod hash;
mod pipeline;

use std::path::PathBuf;

use anyhow::Context;

/// Command-line subcommand selection.
#[derive(Debug)]
enum Subcommand {
    /// Run the full build pipeline using the config at the given path.
    Run {
        config: PathBuf,
        dry_run: bool,
    },
    /// Re-verify an already-built rootfs against its manifest.
    Verify {
        rootfs: PathBuf,
    },
    /// Remove intermediate build artifacts (loop mount points, temp images).
    Clean {
        workdir: PathBuf,
    },
}

/// Top-level argv parse target.
#[derive(Debug)]
struct Args {
    subcommand: Subcommand,
}

/// Parse command-line arguments from `std::env::args()`.
fn parse_args() -> anyhow::Result<Args> {
    parse_args_from(std::env::args().skip(1).collect())
}

/// Parse from an explicit argument list (factored out for unit tests).
fn parse_args_from(argv: Vec<String>) -> anyhow::Result<Args> {
    let mut iter = argv.into_iter();
    let subcmd = iter
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing subcommand: run | verify | clean"))?;

    match subcmd.as_str() {
        "run" => {
            let mut config: Option<PathBuf> = None;
            let mut dry_run = false;
            while let Some(flag) = iter.next() {
                match flag.as_str() {
                    "--config" => {
                        config = Some(
                            iter.next()
                                .ok_or_else(|| anyhow::anyhow!("--config requires a value"))?
                                .into(),
                        );
                    }
                    "--dry-run" => dry_run = true,
                    other => anyhow::bail!("unknown flag for run: {}", other),
                }
            }
            let config =
                config.ok_or_else(|| anyhow::anyhow!("run requires --config <path>"))?;
            Ok(Args {
                subcommand: Subcommand::Run { config, dry_run },
            })
        }
        "verify" => {
            let mut rootfs: Option<PathBuf> = None;
            while let Some(flag) = iter.next() {
                match flag.as_str() {
                    "--rootfs" => {
                        rootfs = Some(
                            iter.next()
                                .ok_or_else(|| anyhow::anyhow!("--rootfs requires a value"))?
                                .into(),
                        );
                    }
                    other => anyhow::bail!("unknown flag for verify: {}", other),
                }
            }
            let rootfs =
                rootfs.ok_or_else(|| anyhow::anyhow!("verify requires --rootfs <path>"))?;
            Ok(Args {
                subcommand: Subcommand::Verify { rootfs },
            })
        }
        "clean" => {
            let mut workdir: Option<PathBuf> = None;
            while let Some(flag) = iter.next() {
                match flag.as_str() {
                    "--workdir" => {
                        workdir = Some(
                            iter.next()
                                .ok_or_else(|| anyhow::anyhow!("--workdir requires a value"))?
                                .into(),
                        );
                    }
                    other => anyhow::bail!("unknown flag for clean: {}", other),
                }
            }
            let workdir =
                workdir.ok_or_else(|| anyhow::anyhow!("clean requires --workdir <path>"))?;
            Ok(Args {
                subcommand: Subcommand::Clean { workdir },
            })
        }
        other => anyhow::bail!(
            "unknown subcommand '{}'; expected: run | verify | clean",
            other
        ),
    }
}

fn run(args: Args) -> anyhow::Result<()> {
    match args.subcommand {
        Subcommand::Run { config, dry_run } => pipeline::run_build(config, dry_run),
        Subcommand::Verify { rootfs } => run_verify(rootfs),
        Subcommand::Clean { workdir } => run_clean(workdir),
    }
}

fn run_verify(rootfs: PathBuf) -> anyhow::Result<()> {
    let manifest_path = {
        let mut p = rootfs.clone().into_os_string();
        p.push(".manifest.json");
        PathBuf::from(p)
    };
    let manifest = m80_image_manifest::Manifest::read(&manifest_path)
        .with_context(|| format!("reading manifest at {}", manifest_path.display()))?;
    let root = rootfs
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    manifest
        .verify(root)
        .with_context(|| format!("verifying manifest at {}", manifest_path.display()))?;
    println!("verified");
    Ok(())
}

fn run_clean(workdir: PathBuf) -> anyhow::Result<()> {
    if !workdir.exists() {
        // Idempotent: nothing to clean.
        return Ok(());
    }
    std::fs::remove_dir_all(&workdir)
        .with_context(|| format!("removing workdir {}", workdir.display()))
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    run(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_run_with_config() {
        let args = parse_args_from(argv(&["run", "--config", "/tmp/foo.toml"])).unwrap();
        match args.subcommand {
            Subcommand::Run { config, dry_run } => {
                assert_eq!(config, PathBuf::from("/tmp/foo.toml"));
                assert!(!dry_run);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_run_dry_run() {
        let args =
            parse_args_from(argv(&["run", "--config", "/tmp/foo.toml", "--dry-run"])).unwrap();
        match args.subcommand {
            Subcommand::Run { dry_run, .. } => assert!(dry_run),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_verify() {
        let args = parse_args_from(argv(&["verify", "--rootfs", "/tmp/r.ext4"])).unwrap();
        match args.subcommand {
            Subcommand::Verify { rootfs } => {
                assert_eq!(rootfs, PathBuf::from("/tmp/r.ext4"));
            }
            _ => panic!("expected Verify"),
        }
    }

    #[test]
    fn parse_clean() {
        let args = parse_args_from(argv(&["clean", "--workdir", "/tmp/work"])).unwrap();
        match args.subcommand {
            Subcommand::Clean { workdir } => {
                assert_eq!(workdir, PathBuf::from("/tmp/work"));
            }
            _ => panic!("expected Clean"),
        }
    }

    #[test]
    fn parse_missing_subcommand_fails() {
        let err = parse_args_from(vec![]).unwrap_err();
        assert!(
            err.to_string().contains("missing subcommand"),
            "expected 'missing subcommand' in: {err}"
        );
    }

    #[test]
    fn parse_unknown_subcommand_fails() {
        let err = parse_args_from(argv(&["frobnicate"])).unwrap_err();
        assert!(
            err.to_string().contains("unknown subcommand"),
            "expected 'unknown subcommand' in: {err}"
        );
    }

    #[test]
    fn parse_run_missing_config_fails() {
        let err = parse_args_from(argv(&["run"])).unwrap_err();
        assert!(
            err.to_string().contains("--config"),
            "expected '--config' in: {err}"
        );
    }

    #[test]
    fn parse_verify_missing_rootfs_fails() {
        let err = parse_args_from(argv(&["verify"])).unwrap_err();
        assert!(
            err.to_string().contains("--rootfs"),
            "expected '--rootfs' in: {err}"
        );
    }

    #[test]
    fn parse_clean_missing_workdir_fails() {
        let err = parse_args_from(argv(&["clean"])).unwrap_err();
        assert!(
            err.to_string().contains("--workdir"),
            "expected '--workdir' in: {err}"
        );
    }
}
