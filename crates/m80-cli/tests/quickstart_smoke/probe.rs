use super::support::*;
use serde_json::Value;

#[test]
fn quickstart_checks_host_substrate_before_active_install_paths() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let dst = dir.path().join("substrate-dst");
    let run_root = dir.path().join("substrate-run");

    let output = m80()
        .env("M80_CGROUP_MODE", "bogus")
        .args([
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid cgroup mode"),
        "quickstart should report the typed substrate config failure; stderr={stderr}"
    );
    assert!(
        !dst.exists(),
        "quickstart must not create active artifact dir before substrate passes"
    );
    assert!(
        !run_root.exists(),
        "quickstart must not create run-root before substrate passes"
    );
}

#[test]
fn quickstart_json_requires_no_run_to_keep_stdout_machine_readable() {
    let output = m80()
        .args([
            "--json",
            "quickstart",
            "--artifact-url",
            "file:///tmp/m80-artifacts.tar.gz",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "JSON quickstart config error should not write stdout"
    );

    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["data"]["variant"], "Config");
    assert!(
        value["data"]["detail"]
            .as_str()
            .unwrap()
            .contains("--json requires --no-run"),
        "unexpected error payload: {value}"
    );
}
