//! Real-KVM lifecycle-failure cleanup coverage.
//!
//! Ignored by default because these tests need a KVM-capable host, real
//! Firecracker artifacts, and timeout budgets long enough to prove stalled
//! guest readiness paths.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use common::RunDirDumpGuard;
use m80_firecracker::{Backend, BackendConfig, CgroupMode, FcError, SandboxConfig, SnapshotPaths};
use m80_proto::GUEST_PORT_DEFAULT;
use m80_proto::{Envelope, ExecRequest, ExecStatus};
use m80_vsock::Channel;

#[test]
#[ignore = "requires-kvm requires-root requires-artifacts"]
fn api_socket_timeout_cleans_partial_state() {
    let fake_dir = tempfile::tempdir().expect("fake firecracker tempdir");
    let fake_firecracker = write_fake_firecracker(fake_dir.path(), "fake-firecracker");
    let mut discovery =
        m80_preflight::run().expect("preflight must pass on a privileged host with m80 artifacts");
    discovery.firecracker_bin = fake_firecracker;
    let backend = Arc::new(Backend::new(make_backend_config(discovery.clone())).unwrap());
    let vm_id = common::unique_vm_id("api-sock-to");
    let run_dir = discovery.run_root.join(&vm_id);

    let sandbox = backend
        .admit(default_config(&vm_id))
        .expect("admit api-socket timeout launch");
    let err = match sandbox.launch() {
        Ok(running) => {
            let stopped = running
                .force_kill()
                .expect("force-kill unexpected api-socket launch");
            stopped
                .delete()
                .expect("delete unexpected api-socket launch");
            panic!("fake Firecracker unexpectedly reached RunningSandbox");
        }
        Err(err) => err,
    };

    assert_api_socket_timeout(err);
    assert!(
        !run_dir.exists(),
        "api socket timeout must move partial run-dir out of live path: {}",
        run_dir.display()
    );
    assert_failed_launch_preserved(&discovery.run_root, &vm_id, "ApiSocketTimeout");

    let admitted_after_timeout = backend
        .admit(default_config("api-sock-to-reuse"))
        .expect("admission permit must be released after api socket timeout");
    drop(admitted_after_timeout);
}

#[test]
#[ignore = "requires-kvm requires-root requires-cgroup-v2 requires-artifacts"]
fn cgroup_create_failure_mid_launch_cleans_partial_state_and_releases_permit() {
    let fake_dir = tempfile::tempdir().expect("fake firecracker tempdir");
    // fc_basename eats into the 107-byte AF_UNIX path budget alongside vm_id;
    // keep the fake binary name short so the path stays under the cap.
    let fake_name = format!("fake-fc-cgr-{:04x}", common::unique_suffix() % 0x10000);
    let fake_firecracker = write_fake_firecracker(fake_dir.path(), &fake_name);
    let mut discovery =
        m80_preflight::run().expect("preflight must pass on a privileged host with m80 artifacts");
    discovery.firecracker_bin = fake_firecracker;
    let backend = Arc::new(
        Backend::new(make_backend_config_with_cgroup(
            discovery.clone(),
            CgroupMode::UnifiedV2,
        ))
        .unwrap(),
    );
    let vm_id = common::unique_vm_id("cgr-fail");
    let run_dir = discovery.run_root.join(&vm_id);
    let _fault = EnvGuard::set("M80_TEST_FAIL_CGROUP_CREATE_FOR_VM", &vm_id);

    let sandbox = backend
        .admit(default_config(&vm_id))
        .expect("admit cgroup failure launch");
    let err = match sandbox.launch() {
        Ok(running) => {
            let stopped = running
                .force_kill()
                .expect("force-kill unexpected cgroup launch");
            stopped.delete().expect("delete unexpected cgroup launch");
            panic!("cgroup fault injection unexpectedly reached RunningSandbox");
        }
        Err(err) => err,
    };

    assert_cgroup_error(err);
    assert!(
        !run_dir.exists(),
        "cgroup create failure must move partial run-dir out of live path: {}",
        run_dir.display()
    );
    assert_failed_launch_preserved(&discovery.run_root, &vm_id, "Cgroup");
    assert_no_process_cmdline_contains(&fake_name);

    let admitted_after_failure = backend
        .admit(default_config("cgr-fail-reuse"))
        .expect("admission permit must be released after cgroup create failure");
    drop(admitted_after_failure);
}

#[test]
#[ignore = "requires-kvm requires-artifacts requires-malicious-artifacts"]
fn guestd_not_ready_timeout_cleans_partial_state() {
    let discovery = malicious_discovery()
        .expect("preflight must pass on a KVM-capable host with malicious m80 artifacts");
    let backend = Arc::new(Backend::new(make_backend_config(discovery.clone())).unwrap());
    let vm_id = common::unique_vm_id("gnr-cold");
    let run_dir = discovery.run_root.join(&vm_id);

    let sandbox = backend
        .admit(no_ready_guestd_config(&vm_id))
        .expect("admit stalled cold launch");
    let err = match sandbox.launch() {
        Ok(running) => {
            let stopped = running.force_kill().expect("force-kill unexpected launch");
            stopped.delete().expect("delete unexpected launch");
            panic!("stalled guestd cold launch unexpectedly reached RunningSandbox");
        }
        Err(err) => err,
    };

    assert_guestd_timeout(err);
    assert!(
        !run_dir.exists(),
        "guestd ready timeout must move partial cold-launch run-dir out of live path: {}",
        run_dir.display()
    );
    assert_failed_launch_preserved(&discovery.run_root, &vm_id, "GuestdReadyTimeout");

    let healthy_discovery =
        m80_preflight::run().expect("preflight must pass for healthy reuse launch");
    let healthy_backend = Arc::new(Backend::new(make_backend_config(healthy_discovery)).unwrap());
    let mut running = launch_healthy(&healthy_backend, "gnr-cold-r");
    assert_exec_ok(&mut running, "printf cold-reuse-ok");
    running
        .stop()
        .expect("stop reuse VM")
        .delete()
        .expect("delete reuse VM");
}

#[test]
#[ignore = "requires-kvm requires-artifacts requires-snapshot-support"]
fn restore_guestd_not_ready_timeout_cleans_partial_state() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let backend = Arc::new(Backend::new(make_backend_config(discovery.clone())).unwrap());
    let snap_dir = discovery.run_root.join(format!(
        "gnr-snap-{:04x}",
        common::unique_suffix() % 0x10000
    ));
    let paths = snapshot_paths(&snap_dir);

    let mut golden = launch_healthy(&backend, "gnr-gold");
    let _golden_dump = RunDirDumpGuard::new(golden.run_dir().to_path_buf());
    let _blocking_exec = start_blocking_exec(&golden, &discovery);
    std::thread::sleep(Duration::from_millis(500));
    golden
        .capture(paths.clone())
        .expect("capture snapshot with busy guestd");
    golden
        .force_kill()
        .expect("force-kill busy-guestd golden")
        .delete()
        .expect("delete busy-guestd golden");

    let restore_vm_id = common::unique_vm_id("gnr-rest");
    let restore_run_dir = discovery.run_root.join(&restore_vm_id);
    let sandbox = backend
        .admit(default_config(&restore_vm_id))
        .expect("admit restore timeout launch");
    let err = match sandbox.launch_from_snapshot(paths, &discovery) {
        Ok(running) => {
            let stopped = running.force_kill().expect("force-kill unexpected restore");
            stopped.delete().expect("delete unexpected restore");
            panic!("stalled guestd restore unexpectedly reached RunningSandbox");
        }
        Err(err) => err,
    };

    assert_guestd_timeout(err);
    assert!(
        !restore_run_dir.exists(),
        "guestd ready timeout must move partial restore run-dir out of live path: {}",
        restore_run_dir.display()
    );
    assert_failed_launch_preserved(&discovery.run_root, &restore_vm_id, "GuestdReadyTimeout");

    let mut running = launch_healthy(&backend, "gnr-rest-r");
    assert_exec_ok(&mut running, "printf restore-reuse-ok");
    running
        .stop()
        .expect("stop restore reuse VM")
        .delete()
        .expect("delete restore reuse VM");

    let _ = std::fs::remove_dir_all(snap_dir);
}

fn make_backend_config(discovery: m80_preflight::Discovery) -> BackendConfig {
    make_backend_config_with_cgroup(discovery, CgroupMode::Disabled)
}

fn make_backend_config_with_cgroup(
    discovery: m80_preflight::Discovery,
    cgroup_mode: CgroupMode,
) -> BackendConfig {
    let run_root = discovery.run_root.clone();
    BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(jail_id_from_env("M80_JAIL_UID", 3000))
        .jail_gid(jail_id_from_env("M80_JAIL_GID", 3000))
        .cgroup_mode(cgroup_mode)
        .build()
}

fn jail_id_from_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn default_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.to_owned()),
        vcpu_count: Some(m80_firecracker::FIRST_LINE_VCPU_COUNT),
        mem_size_mib: Some(m80_firecracker::FIRST_LINE_MEM_SIZE_MIB),
        huge_pages_2m: false,
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        ..common::sandbox_config()
    }
}

fn no_ready_guestd_config(vm_id: &str) -> SandboxConfig {
    SandboxConfig {
        boot_args: Some("m80.malicious_attack=no_ready".into()),
        ..default_config(vm_id)
    }
}

fn malicious_discovery() -> Result<m80_preflight::Discovery, m80_preflight::PreflightError> {
    let artifact_dir = std::env::var_os("M80_MALICIOUS_ARTIFACT_DIR")
        .map(PathBuf::from)
        .expect("M80_MALICIOUS_ARTIFACT_DIR must point at no-ready guestd image artifacts");
    m80_preflight::run_with_configs(
        m80_preflight::BinaryDiscoveryConfig::from_env(),
        m80_preflight::ArtifactPreflightConfig {
            kernel_image: Some(artifact_dir.join("vmlinux")),
            artifact_dir: artifact_dir.clone(),
            rootfs_image: Some(artifact_dir.join("output.ext4")),
            kernel_kind: None,
            ..m80_preflight::ArtifactPreflightConfig::from_env()
        },
        m80_preflight::HostFeaturePreflightConfig {
            cgroup_mode: m80_preflight::CgroupPreflightMode::Disabled,
            jail_uid: jail_id_from_env("M80_JAIL_UID", 3000),
            jail_gid: jail_id_from_env("M80_JAIL_GID", 3000),
            expected_concurrent_vms: 1,
        },
    )
}

fn launch_healthy(backend: &Arc<Backend>, prefix: &str) -> m80_firecracker::RunningSandbox {
    let vm_id = common::unique_vm_id(prefix);
    backend
        .admit(default_config(&vm_id))
        .expect("admit healthy launch")
        .launch()
        .expect("healthy launch after failure cleanup")
}

fn snapshot_paths(dir: &Path) -> SnapshotPaths {
    std::fs::create_dir_all(dir).expect("create snapshot dir");
    SnapshotPaths {
        vm_state: dir.join("vm.snap"),
        mem: dir.join("mem.snap"),
    }
}

fn write_fake_firecracker(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let source = dir.join(format!("{name}.c"));
    // The real jailer chroots before execing Firecracker, so a shell script
    // fake would fail unless the jail root also carried `/bin/sh` and its
    // loader/libs. Compile a tiny static ELF that writes the same pid file
    // Firecracker writes inside the jail and then stays alive without creating
    // the API socket. The official jailer writes the host-visible
    // `<exec-basename>.pid` before handing off to this binary.
    std::fs::write(
        &source,
        r#"#include <signal.h>
	#include <unistd.h>

	int main(void) {
	    for (;;) {
	        pause();
	    }
}
"#,
    )
    .expect("write fake firecracker source");
    let output = Command::new("cc")
        .arg("-static")
        .arg("-O2")
        .arg(&source)
        .arg("-o")
        .arg(&path)
        .output()
        .expect("run cc for fake firecracker");
    assert!(
        output.status.success(),
        "compile fake firecracker failed: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut perms = std::fs::metadata(&path)
        .expect("fake firecracker metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod fake firecracker");
    path
}

fn assert_exec_ok(running: &mut m80_firecracker::RunningSandbox, script: &str) {
    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .unwrap_or_else(|e| panic!("exec {script:?}: {e}"));
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "exec {script:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );
}

fn start_blocking_exec(
    running: &m80_firecracker::RunningSandbox,
    discovery: &m80_preflight::Discovery,
) -> Channel {
    let vsock = m80_firecracker::vsock_socket_path(running.run_dir(), &discovery.firecracker_bin);
    let mut channel =
        Channel::open_uds_only(&vsock, GUEST_PORT_DEFAULT).expect("open blocking exec channel");
    let req = ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "sleep 60".into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(60_000),
        streaming: true,
    };
    channel
        .send(&Envelope::with_request_id(req, "restore-blocking-exec"))
        .expect("send blocking exec request");
    channel
}

fn assert_api_socket_timeout(err: FcError) {
    assert!(
        matches!(err, FcError::ApiSocketTimeout { .. }),
        "expected ApiSocketTimeout, got {err:?}"
    );
}

fn assert_guestd_timeout(err: FcError) {
    assert!(
        matches!(err, FcError::GuestdReadyTimeout { .. }),
        "expected GuestdReadyTimeout, got {err:?}"
    );
}

fn assert_cgroup_error(err: FcError) {
    assert!(
        matches!(err, FcError::Cgroup(_)),
        "expected Cgroup error, got {err:?}"
    );
}

fn assert_failed_launch_preserved(run_root: &Path, vm_id: &str, expected_variant: &str) {
    let preserved_parent = run_root.join(".preserved");
    let preserved = std::fs::read_dir(&preserved_parent)
        .unwrap_or_else(|err| panic!("read {}: {err}", preserved_parent.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(vm_id))
        })
        .unwrap_or_else(|| panic!("no preserved run-dir found for {vm_id}"));

    let summary_path = preserved.join("failure_summary.json");
    let summary: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&summary_path)
            .unwrap_or_else(|err| panic!("read {}: {err}", summary_path.display())),
    )
    .unwrap_or_else(|err| panic!("parse {}: {err}", summary_path.display()));
    assert_eq!(summary["vm_id"], vm_id);
    assert_eq!(summary["error_variant"], expected_variant);
    assert!(summary["failed_phase"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert!(
        summary["error_display"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "summary missing error_display: {summary}"
    );
}

fn assert_no_process_cmdline_contains(needle: &str) {
    let mut matches = Vec::new();
    let entries = std::fs::read_dir("/proc").expect("read /proc");
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let cmdline = entry.path().join("cmdline");
        let Ok(bytes) = std::fs::read(&cmdline) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes).replace('\0', " ");
        if text.contains(needle) {
            matches.push(format!("{}: {text}", entry.path().display()));
        }
    }
    assert!(
        matches.is_empty(),
        "launch failure cleanup left fake Firecracker process(es): {matches:?}"
    );
}

// vm_ids must stay short: the AF_UNIX socket path
// `<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock` is capped at
// 107 bytes by the kernel, and m80-jailer's nested layout uses vm_id twice.
// With run_root=/var/lib/m80-run (16) and fc_basename="firecracker" (11), V<=27;
// the fake-firecracker cases tighten this further. Prefixes here are kept under
// ~17 chars and the suffix is 4 hex digits to leave headroom for both.

struct EnvGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(old) => std::env::set_var(self.key, old),
            None => std::env::remove_var(self.key),
        }
    }
}
