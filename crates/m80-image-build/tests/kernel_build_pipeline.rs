//! Pipeline-level tests for the stripped-kernel build path.
//!
//! The actual Docker build is skipped in CI (requires Docker daemon and
//! network access). Config-sha computation has a unit test that runs
//! unconditionally against the committed m80-stripped.config.

use std::io::Read;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// Compute sha256 of a file; mirrors the logic in `pipeline::config_sha_from_file`.
fn sha256_of_file(path: &std::path::Path) -> String {
    let mut file = std::fs::File::open(path).expect("file must exist");
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).expect("read must succeed");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    hex::encode(hasher.finalize())
}

fn kernel_builder_config() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/m80-image-build at test time.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set (run via cargo test)");
    PathBuf::from(manifest_dir)
        .join("kernel-builder")
        .join("m80-stripped.config")
}

fn committed_config_text() -> String {
    std::fs::read_to_string(kernel_builder_config()).expect("m80-stripped.config must be readable")
}

/// The committed m80-stripped.config must exist and produce a 64-hex-char sha256.
/// This pins the config-sha computation logic that gates the vmlinux filename.
#[test]
fn config_sha_of_committed_config_is_hex64() {
    let config_path = kernel_builder_config();
    assert!(
        config_path.exists(),
        "m80-stripped.config not found at {} — was the file committed?",
        config_path.display()
    );
    let sha = sha256_of_file(&config_path);
    assert_eq!(sha.len(), 64, "sha256 must be 64 hex chars, got: {sha}");
    assert!(
        sha.chars().all(|c| c.is_ascii_hexdigit()),
        "sha256 must be lowercase hex, got: {sha}"
    );
}

/// Config sha must be deterministic: two calls on the same file produce equal output.
#[test]
fn config_sha_is_deterministic() {
    let config_path = kernel_builder_config();
    if !config_path.exists() {
        return; // missing file is covered by the test above
    }
    let sha1 = sha256_of_file(&config_path);
    let sha2 = sha256_of_file(&config_path);
    assert_eq!(sha1, sha2, "config sha must be deterministic");
}

/// Mutating the config content produces a different sha (basic collision sanity check).
#[test]
fn config_sha_changes_when_content_changes() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("config_a");
    let p2 = dir.path().join("config_b");
    std::fs::write(&p1, b"CONFIG_FOO=y\n").unwrap();
    std::fs::write(&p2, b"CONFIG_FOO=n\n").unwrap();
    assert_ne!(
        sha256_of_file(&p1),
        sha256_of_file(&p2),
        "different configs must produce different shas"
    );
}

#[test]
fn stripped_config_keeps_overlayfs_built_in() {
    let cfg = committed_config_text();
    assert!(
        cfg.lines().any(|line| line == "CONFIG_OVERLAY_FS=y"),
        "storage pivot requires built-in overlayfs, not a module or omitted symbol"
    );
    assert!(
        !cfg.lines().any(|line| line == "CONFIG_OVERLAY_FS=m"),
        "overlayfs must be built in because PID 1 mounts overlayfs before modules are available"
    );
}

#[test]
fn stripped_config_keeps_overlay_xino_auto_built_in() {
    let cfg = committed_config_text();
    assert!(
        cfg.lines()
            .any(|line| line == "CONFIG_OVERLAY_FS_XINO_AUTO=y"),
        "storage pivot requires xino auto for stable cross-layer inode behavior"
    );
    assert!(
        !cfg.lines()
            .any(|line| line == "CONFIG_OVERLAY_FS_XINO_AUTO=m"),
        "xino auto must be built in with overlayfs"
    );
}

#[test]
fn stripped_config_disables_smp_for_single_vcpu_shape() {
    let cfg = committed_config_text();
    assert!(
        cfg.lines().any(|line| line == "# CONFIG_SMP is not set"),
        "stripped kernel must compile out SMP instead of relying on runtime nosmp"
    );
    assert!(
        !cfg.lines()
            .any(|line| line == "CONFIG_SMP=y" || line == "CONFIG_SMP=m"),
        "SMP must not be built into the stripped kernel"
    );
}

#[test]
fn kernel_build_script_strips_symbol_tables_before_publish() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set (run via cargo test)");
    let build_script = PathBuf::from(manifest_dir)
        .join("kernel-builder")
        .join("build.sh");
    let script = std::fs::read_to_string(build_script).unwrap();
    let strip_idx = script
        .find("strip --strip-all vmlinux")
        .expect("build.sh must strip symbol tables from vmlinux");
    let copy_idx = script
        .find("cp vmlinux")
        .expect("build.sh must copy vmlinux to /out");
    assert!(
        strip_idx < copy_idx,
        "symbol tables must be stripped before publishing vmlinux"
    );
}

/// `build_stripped_kernel` is ignored in CI (Docker + network required).
/// Run manually with `cargo test -- --ignored` or `--include-ignored`.
#[test]
#[ignore = "requires Docker daemon + network access; run manually with `-- --ignored`"]
fn build_stripped_kernel_smoke() {
    use assert_cmd::Command;

    let workspace_root = workspace_root();

    let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
    cmd.args(["kernel", "build", "--workspace"])
        .arg(&workspace_root);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "kernel build should exit 0; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The output line must reference a vmlinux-m80-<64 hex chars>.bin file.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let vmlinux_line = stdout
        .lines()
        .find(|l| l.starts_with("vmlinux: "))
        .expect("stdout must contain 'vmlinux: <path>' line");
    let path_str = vmlinux_line.strip_prefix("vmlinux: ").unwrap();
    assert!(
        path_str.contains("vmlinux-m80-"),
        "output path must contain 'vmlinux-m80-': {path_str}"
    );
    assert!(
        path_str.ends_with(".bin"),
        "output path must end with '.bin': {path_str}"
    );
    assert!(
        std::path::Path::new(path_str).exists(),
        "reported vmlinux path must exist on disk: {path_str}"
    );
}

#[test]
#[ignore = "requires Docker daemon + network access; run manually with `-- --ignored`"]
fn stripped_kernel_build_is_deterministic() {
    let workspace_root = workspace_root();

    let first = run_kernel_build(&workspace_root);
    let first_sha = sha256_of_file(&first);
    let second = run_kernel_build(&workspace_root);
    let second_sha = sha256_of_file(&second);

    assert_eq!(
        first, second,
        "same resolved kernel config must report the same vmlinux path"
    );
    assert_eq!(
        first_sha, second_sha,
        "same resolved kernel config must produce byte-identical vmlinux"
    );
}

fn workspace_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run_kernel_build(workspace_root: &std::path::Path) -> PathBuf {
    use assert_cmd::Command;

    let mut cmd = Command::cargo_bin("m80-image-build").unwrap();
    cmd.args(["kernel", "build", "--workspace"])
        .arg(workspace_root);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "kernel build should exit 0; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path_str = stdout
        .lines()
        .find_map(|line| line.strip_prefix("vmlinux: "))
        .expect("stdout must contain 'vmlinux: <path>' line");
    let path = PathBuf::from(path_str);
    assert!(
        path.exists(),
        "reported vmlinux path must exist on disk: {}",
        path.display()
    );
    path
}
