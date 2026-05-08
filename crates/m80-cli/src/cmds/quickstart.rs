use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use m80_firecracker::FcError;

use crate::args::QuickstartArgs;
use crate::errors;
use crate::json;

const REQUIRED_ARTIFACTS: &[&str] = &[
    "vmlinux",
    "output.ext4",
    "output.ext4.manifest.json",
    "m80-guestd",
];

pub(crate) fn cmd_quickstart(args: QuickstartArgs, json_output: bool) -> anyhow::Result<i32> {
    if json_output && !args.no_run {
        let err = FcError::config_other(
            "m80 quickstart --json requires --no-run so stdout remains machine-readable".to_owned(),
        );
        return Ok(errors::render_error(&err, json_output));
    }

    let artifact_dir = args.artifact_dir.unwrap_or_else(default_artifact_dir);
    let run_root = args.run_root.unwrap_or_else(default_run_root);

    match run_quickstart(
        &args.artifact_url,
        &artifact_dir,
        &run_root,
        args.no_run,
        json_output,
    ) {
        Ok(summary) => {
            if json_output {
                println!("{}", summary_json(&summary));
            } else {
                eprintln!(
                    "installed artifacts under {}",
                    summary.artifact_dir.display()
                );
                print_next_steps(&summary);
            }
            Ok(0)
        }
        Err(e) => Ok(errors::render_error(&e, json_output)),
    }
}

struct QuickstartSummary {
    artifact_dir: PathBuf,
    run_root: PathBuf,
    ran_probe: bool,
}

fn run_quickstart(
    artifact_url: &str,
    artifact_dir: &Path,
    run_root: &Path,
    no_run: bool,
    json_output: bool,
) -> Result<QuickstartSummary, FcError> {
    let temp = TempTree::new()?;
    let tarball = temp.path().join("artifacts.tar.gz");
    let extract_dir = temp.path().join("artifacts");
    fs::create_dir(&extract_dir).map_err(|e| {
        FcError::config_other(format!(
            "creating quickstart extract dir {}: {e}",
            extract_dir.display()
        ))
    })?;

    if !json_output {
        eprintln!("downloading m80 artifacts: {artifact_url}");
    }
    run_status(
        Command::new("curl")
            .arg("-fsSL")
            .arg(artifact_url)
            .arg("-o")
            .arg(&tarball),
        "curl artifact tarball",
    )?;
    run_status(
        Command::new("tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&extract_dir),
        "extract artifact tarball",
    )?;

    let sums = extract_dir.join("SHA256SUMS");
    if !sums.is_file() {
        return Err(FcError::config_other(
            "artifact tarball is missing SHA256SUMS".to_owned(),
        ));
    }
    run_output(
        Command::new("sha256sum")
            .arg("-c")
            .arg("SHA256SUMS")
            .current_dir(&extract_dir),
        "verify artifact checksums",
        json_output,
    )?;

    for file in REQUIRED_ARTIFACTS {
        let path = extract_dir.join(file);
        if !path.is_file() {
            return Err(FcError::config_other(format!(
                "artifact tarball is missing {file}"
            )));
        }
    }

    fs::create_dir_all(artifact_dir).map_err(|e| {
        FcError::config_other(format!(
            "creating artifact dir {}: {e}",
            artifact_dir.display()
        ))
    })?;
    fs::create_dir_all(run_root).map_err(|e| {
        FcError::config_other(format!("creating run-root {}: {e}", run_root.display()))
    })?;

    for file in REQUIRED_ARTIFACTS {
        copy_artifact(&extract_dir.join(file), &artifact_dir.join(file))?;
    }
    relocate_manifest(artifact_dir)?;

    if !no_run {
        run_echo_probe(artifact_dir, run_root)?;
    }

    Ok(QuickstartSummary {
        artifact_dir: artifact_dir.to_path_buf(),
        run_root: run_root.to_path_buf(),
        ran_probe: !no_run,
    })
}

fn copy_artifact(src: &Path, dst: &Path) -> Result<(), FcError> {
    fs::copy(src, dst).map_err(|e| {
        FcError::config_other(format!(
            "copying artifact {} -> {}: {e}",
            src.display(),
            dst.display()
        ))
    })?;
    Ok(())
}

fn relocate_manifest(artifact_dir: &Path) -> Result<(), FcError> {
    let manifest_path = artifact_dir.join("output.ext4.manifest.json");
    let mut manifest = m80_image_manifest::Manifest::read(&manifest_path)
        .map_err(|e| FcError::config_other(format!("reading installed manifest: {e}")))?;
    manifest.kernel_image = artifact_dir.join("vmlinux");
    manifest.output_rootfs_image = artifact_dir.join("output.ext4");
    manifest.daemon_binary_path = artifact_dir.join("m80-guestd");
    if manifest.source_rootfs_image.is_some() {
        manifest.source_rootfs_image = Some(artifact_dir.join("source.ext4"));
    }
    manifest
        .write(&manifest_path)
        .map_err(|e| FcError::config_other(format!("writing relocated manifest: {e}")))?;
    manifest
        .verify(artifact_dir)
        .map_err(|e| FcError::config_other(format!("verifying relocated manifest: {e}")))?;
    Ok(())
}

fn run_echo_probe(artifact_dir: &Path, run_root: &Path) -> Result<(), FcError> {
    let current = std::env::current_exe()
        .map_err(|e| FcError::config_other(format!("resolving current m80 binary: {e}")))?;
    let status = Command::new(current)
        .arg("run")
        .arg("--egress")
        .arg("none")
        .arg("--")
        .arg("echo")
        .arg("hello")
        .env("M80_KERNEL_IMAGE", artifact_dir.join("vmlinux"))
        .env("M80_ROOTFS_IMAGE", artifact_dir.join("output.ext4"))
        .env("M80_RUN_ROOT", run_root)
        .status()
        .map_err(|e| FcError::config_other(format!("spawning `m80 run -- echo hello`: {e}")))?;
    if !status.success() {
        return Err(FcError::config_other(format!(
            "`m80 run -- echo hello` failed with exit {:?}",
            status.code()
        )));
    }
    Ok(())
}

fn run_status(cmd: &mut Command, label: &str) -> Result<(), FcError> {
    let status = cmd
        .status()
        .map_err(|e| FcError::config_other(format!("spawning {label}: {e}")))?;
    if !status.success() {
        return Err(FcError::config_other(format!(
            "{label} failed with exit {:?}",
            status.code()
        )));
    }
    Ok(())
}

fn run_output(cmd: &mut Command, label: &str, json_output: bool) -> Result<(), FcError> {
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| FcError::config_other(format!("spawning {label}: {e}")))?;
    if !json_output {
        print_command_output(&output.stdout);
        print_command_output(&output.stderr);
    }
    if !output.status.success() {
        return Err(FcError::config_other(format!(
            "{label} failed with exit {:?}: {}{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

fn print_command_output(bytes: &[u8]) {
    if !bytes.is_empty() {
        eprint!("{}", String::from_utf8_lossy(bytes));
    }
}

fn summary_json(summary: &QuickstartSummary) -> String {
    let kernel = summary.artifact_dir.join("vmlinux");
    let rootfs = summary.artifact_dir.join("output.ext4");
    let manifest = summary.artifact_dir.join("output.ext4.manifest.json");
    let guestd = summary.artifact_dir.join("m80-guestd");
    let obj = serde_json::json!({
        "artifact_dir": summary.artifact_dir.display().to_string(),
        "kernel_image": kernel.display().to_string(),
        "rootfs_image": rootfs.display().to_string(),
        "manifest": manifest.display().to_string(),
        "guestd": guestd.display().to_string(),
        "run_root": summary.run_root.display().to_string(),
        "ran_probe": summary.ran_probe,
    });
    json::to_pretty(&obj)
}

fn print_next_steps(summary: &QuickstartSummary) {
    let kernel = summary.artifact_dir.join("vmlinux");
    let rootfs = summary.artifact_dir.join("output.ext4");
    let run_root = &summary.run_root;
    eprintln!(
        "\nNext:\n  M80_KERNEL_IMAGE={} M80_ROOTFS_IMAGE={} M80_RUN_ROOT={} m80 run --workspace . --cwd /workspace -- ls\n  M80_KERNEL_IMAGE={} M80_ROOTFS_IMAGE={} M80_RUN_ROOT={} m80 run --egress none -- echo isolated\n  M80_KERNEL_IMAGE={} M80_ROOTFS_IMAGE={} M80_RUN_ROOT={} m80 run -it --workspace . -- sh",
        kernel.display(),
        rootfs.display(),
        run_root.display(),
        kernel.display(),
        rootfs.display(),
        run_root.display(),
        kernel.display(),
        rootfs.display(),
        run_root.display(),
    );
}

fn default_artifact_dir() -> PathBuf {
    env_path("M80_ARTIFACT_DIR", "/opt/m80/artifacts")
}

fn default_run_root() -> PathBuf {
    env_path("M80_RUN_ROOT", "/var/run/m80")
}

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new() -> Result<Self, FcError> {
        let path = std::env::temp_dir().join(format!("m80-quickstart-{}", ulid::Ulid::new()));
        fs::create_dir(&path).map_err(|e| {
            FcError::config_other(format!(
                "creating quickstart temp dir {}: {e}",
                path.display()
            ))
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
