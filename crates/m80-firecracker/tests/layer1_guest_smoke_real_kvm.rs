//! Real-KVM Layer 1 guest-visible security smoke probes.
//!
//! The probes are intentionally run from inside the guest. A tiny static C
//! helper covers the privileged-instruction cases that cannot live in Rust
//! because the workspace forbids `unsafe`.

mod common;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecResponse, ExecStatus};

use common::RunDirDumpGuard;

const GUEST_PROBE_PATH: &str = "/tmp/m80-layer1-probe";

struct ProbeCase {
    name: &'static str,
    expected: &'static [&'static str],
}

const PROBES: &[ProbeCase] = &[
    ProbeCase {
        name: "dev_mem_nonroot",
        expected: &[
            "EACCES",
            "EPERM",
            "ENOENT",
            "Permission denied",
            "No such file",
        ],
    },
    ProbeCase {
        name: "virtio_config_write",
        expected: &[
            "EFAULT",
            "EBADF",
            "EINVAL",
            "EPERM",
            "EACCES",
            "EROFS",
            "ENODEV",
            "ENOENT",
            "Permission denied",
            "Bad address",
            "Read-only",
            "Invalid argument",
            "No such device",
            "No such file",
            "no virtio config file exposed",
        ],
    },
    ProbeCase {
        name: "rdmsr_host_msr",
        expected: &["signal="],
    },
    ProbeCase {
        name: "write_cr0",
        expected: &["signal="],
    },
    ProbeCase {
        name: "write_cr4",
        expected: &["signal="],
    },
    ProbeCase {
        name: "proc_kcore_nonroot",
        expected: &[
            "EACCES",
            "EPERM",
            "ENOENT",
            "Permission denied",
            "No such file",
        ],
    },
    ProbeCase {
        name: "ioperm_iopl_nonroot",
        expected: &[
            "EPERM",
            "Operation not permitted",
            "ioperm=EPERM iopl=EPERM",
        ],
    },
    ProbeCase {
        name: "vsock_non_allowed_cid",
        expected: &[
            "ECONNREFUSED",
            "ENODEV",
            "ENETUNREACH",
            "ETIMEDOUT",
            "EADDRNOTAVAIL",
            "Connection refused",
            "No such device",
            "Network is unreachable",
            "timeout",
        ],
    },
];

fn launch_no_egress_vm() -> (m80_firecracker::RunningSandbox, PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(common::unique_vm_id("layer1-guest-smoke")),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_owned();
    (running, run_dir)
}

fn compile_probe_binary() -> Vec<u8> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = crate_dir.join("tests/fixtures/layer1_probe.c");
    let build_dir = tempfile::tempdir().expect("tempdir for layer1 probe build");
    let output = build_dir.path().join("m80-layer1-probe");

    let status = Command::new("cc")
        .arg("-std=c11")
        .arg("-O2")
        .arg("-static")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-o")
        .arg(&output)
        .arg(&source)
        .status()
        .unwrap_or_else(|err| panic!("spawn cc for {}: {err}", source.display()));
    assert!(
        status.success(),
        "compile {} with cc -static failed: {status}",
        source.display()
    );

    std::fs::read(&output)
        .unwrap_or_else(|err| panic!("read compiled probe {}: {err}", output.display()))
}

fn probe_request(name: &str) -> ExecRequest {
    ExecRequest {
        program: GUEST_PROBE_PATH.into(),
        args: vec![name.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(5_000),
        streaming: false,
    }
}

fn response_text(response: &ExecResponse) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    )
}

fn probe_failure(case: &ProbeCase, response: &ExecResponse) -> Option<String> {
    let text = response_text(response);
    if response.status != ExecStatus::Completed {
        return Some(format!(
            "{} did not complete: status={:?}; exit={:?}; {text}",
            case.name, response.status, response.exit_code
        ));
    }
    if response.exit_code == Some(0) {
        return Some(format!("{} reported a breach: {text}", case.name));
    }
    if !text.contains(&format!("BLOCKED {}", case.name)) {
        return Some(format!(
            "{} did not report BLOCKED evidence: {text}",
            case.name
        ));
    }
    if !case.expected.iter().any(|needle| text.contains(needle)) {
        return Some(format!(
            "{} did not include an expected denial token {:?}: {text}",
            case.name, case.expected
        ));
    }
    None
}

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn layer1_guest_known_cve_primitives_are_blocked_in_guest() {
    let probe = compile_probe_binary();
    let (mut running, run_dir) = launch_no_egress_vm();
    let _dump_guard = RunDirDumpGuard::new(run_dir.clone());

    running
        .upload_file_chunked(
            GUEST_PROBE_PATH,
            Some(0o755),
            Cursor::new(probe),
            1024 * 1024,
        )
        .expect("upload layer1 probe");

    let mut failures = Vec::new();
    for case in PROBES {
        match running.exec(probe_request(case.name)) {
            Ok(response) => {
                if let Some(failure) = probe_failure(case, &response) {
                    failures.push(failure);
                }
            }
            Err(err) => failures.push(format!("{} exec failed: {err:?}", case.name)),
        }
    }

    if failures.is_empty() {
        let stopped = running.stop().expect("stop");
        stopped.delete().expect("delete");
        return;
    }

    let mut stderr = std::io::stderr().lock();
    let _ = common::dump_run_dir(&run_dir, 100, &mut stderr);
    let stopped = running.stop().expect("stop");
    let preserved = stopped.preserve_for_triage().expect("preserve run dir");
    panic!(
        "Layer 1 guest smoke failures:\n{}\npreserved run dir={}",
        failures.join("\n"),
        preserved.display()
    );
}

#[test]
fn layer1_probe_fixture_compiles_static_on_host() {
    let probe = compile_probe_binary();
    assert!(
        !probe.is_empty(),
        "compiled layer1 probe fixture must not be empty"
    );
}

#[test]
fn layer1_probe_catalog_names_are_unique() {
    let mut names = std::collections::BTreeSet::new();
    for case in PROBES {
        assert!(names.insert(case.name), "duplicate probe {}", case.name);
    }
}

#[test]
fn layer1_probe_fixture_path_is_under_crate_tests() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layer1_probe.c");
    assert!(path.exists(), "{} must exist", path.display());
}
