//! Persistent-VM sequential exec tests.
//!
//! Verifies that a single `RunningSandbox` can serve multiple `exec` calls
//! in sequence with filesystem state persisting across calls.
//!
//! These tests require a KVM-capable host with real Firecracker + jailer
//! binaries and a built m80 guest image. They are `#[ignore]`d by default
//! and must be run explicitly on a prepared host:
//!
//! ```text
//! sudo CARGO_TARGET_DIR=/tmp/m80-build/target-w2-3 \
//!   cargo test -p m80-firecracker --test persistent_state -- --ignored
//! ```
//!
//! Set the same environment variables as `end_to_end_real_kvm.rs`:
//! `M80_FIRECRACKER_BIN`, `M80_JAILER_BIN`, `M80_KERNEL_IMAGE`,
//! `M80_ROOTFS_IMAGE`, `M80_RUN_ROOT`.

mod common;

use std::sync::mpsc;

use common::RunDirDumpGuard;

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_backend() -> (std::sync::Arc<m80_firecracker::Backend>, std::path::PathBuf) {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = m80_firecracker::BackendConfig::builder(discovery)
        .max_concurrent_vms(4)
        .run_root(run_root.clone())
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(m80_firecracker::CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(m80_firecracker::Backend::new(config).expect("Backend::new"));
    (backend, run_root)
}

fn sandbox_config(vm_id: &str) -> m80_firecracker::SandboxConfig {
    m80_firecracker::SandboxConfig {
        vcpu_count: Some(1),
        mem_size_mib: Some(512),
        huge_pages_2m: false,
        cpuset_cpus: None,
        cpu_template: None,
        fc_log_level: None,
        overlay_size_bytes: 128 * 1024 * 1024,
        overlay_clone_mode: Default::default(),
        ..common::sandbox_config_with_id(vm_id)
    }
}

fn sandbox_config_with_workspace(
    vm_id: &str,
    workspace: std::path::PathBuf,
) -> m80_firecracker::SandboxConfig {
    m80_firecracker::SandboxConfig {
        workspace: Some(workspace),
        ..sandbox_config(vm_id)
    }
}

fn exec_sh(running: &mut m80_firecracker::RunningSandbox, cmd: &str) -> m80_proto::ExecResponse {
    running
        .exec(m80_proto::ExecRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), cmd.into()],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(10_000),
            streaming: false,
        })
        .expect("exec")
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Guest filesystem state persists between two sequential exec calls on the same VM.
///
/// exec1 writes a marker; exec2 reads it back.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn two_execs_filesystem_state_persists() {
    let (backend, run_root) = make_backend();
    let vm_id = "persist-fs";
    // run_dir is <run_root>/<vm_id>/ — established by phase_1 inside launch.
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");

    let work_dir = "/tmp/persist-fs-work";
    assert!(running
        .create_dir(work_dir, Some(0o777), false)
        .expect("create exec-visible persistent-state work dir"));
    let marker = format!("{work_dir}/marker");

    // exec1: create the marker.
    let r1 = exec_sh(&mut running, &format!("touch {marker} && echo created"));
    assert_eq!(
        r1.exit_code,
        Some(0),
        "exec1 must succeed: stderr={}",
        String::from_utf8_lossy(&r1.stderr)
    );
    let out1 = String::from_utf8_lossy(&r1.stdout);
    assert!(out1.contains("created"), "exec1 stdout: {out1:?}");

    // exec2: verify the marker survived.
    let r2 = exec_sh(&mut running, &format!("ls {marker}"));
    assert_eq!(
        r2.exit_code,
        Some(0),
        "exec2 must find marker: stderr={}",
        String::from_utf8_lossy(&r2.stderr)
    );
    let out2 = String::from_utf8_lossy(&r2.stdout);
    assert!(
        out2.contains(&marker),
        "exec2 stdout should contain {marker}, got: {out2:?}"
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

/// Workspace scratch image persists between two sequential exec calls.
///
/// exec1 writes `/workspace/ws_file.txt`; exec2 reads it back.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn two_execs_workspace_persists() {
    let (backend, run_root) = make_backend();
    let vm_id = "persist-workspace";
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let host_workspace = tempfile::tempdir().expect("tempdir");
    let sandbox = backend
        .admit(sandbox_config_with_workspace(
            vm_id,
            host_workspace.path().to_path_buf(),
        ))
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");

    // exec1: write to the in-VM workspace mount.
    let r1 = exec_sh(
        &mut running,
        "echo hello-workspace > /workspace/ws_file.txt && echo ok",
    );
    assert_eq!(r1.exit_code, Some(0), "exec1 must succeed");

    // exec2: read back.
    let r2 = exec_sh(&mut running, "cat /workspace/ws_file.txt");
    assert_eq!(r2.exit_code, Some(0), "exec2 must succeed");
    let out2 = String::from_utf8_lossy(&r2.stdout);
    assert!(
        out2.contains("hello-workspace"),
        "exec2 must see workspace file written by exec1, got: {out2:?}"
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn extract_changes_after_unclean_stop_coherent() {
    let (backend, run_root) = make_backend();
    let vm_id = "extract-after-cancel";
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let host_workspace = tempfile::tempdir().expect("workspace tempdir");
    let sandbox = backend
        .admit(sandbox_config_with_workspace(
            vm_id,
            host_workspace.path().to_path_buf(),
        ))
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");

    let (cancel_tx, cancel_rx) = mpsc::channel();
    let mut stdout_total = 0usize;
    let mut cancel_sent = false;
    let exit = running
        .exec_streaming_with_cancel(
            m80_proto::ExecRequest {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "set -eu; \
                     printf 'stable-before-cancel\\n' > /workspace/stable.txt; \
                     i=0; \
                     while :; do \
                       printf 'm80-cancel-stream-%06d\\n' \"$i\"; \
                       printf 'workspace-%06d\\n' \"$i\" >> /workspace/partial.log; \
                       i=$((i + 1)); \
                     done"
                        .into(),
                ],
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: Some(30_000),
                streaming: false,
            },
            cancel_rx,
            |chunk| {
                if let m80_firecracker::ExecChunk::Stdout { bytes, .. } = chunk {
                    stdout_total = stdout_total.saturating_add(bytes.len());
                    if stdout_total >= 64 * 1024 && !cancel_sent {
                        cancel_tx.send(()).expect("send cancel");
                        cancel_sent = true;
                    }
                }
                Ok(())
            },
        )
        .expect("streaming exec cancel should return terminal status");

    assert!(cancel_sent, "streaming exec never emitted enough stdout");
    assert_eq!(exit.status, m80_proto::ExecStatus::Cancelled);

    let stopped = running.stop().expect("stop after cancelled streaming exec");
    let extract_parent = tempfile::tempdir().expect("extract parent tempdir");
    let extracted = extract_parent.path().join("changes");
    match stopped.extract_changes(&extracted) {
        Ok(change_set) => {
            assert!(
                change_set
                    .staged
                    .iter()
                    .any(|path| path == std::path::Path::new("stable.txt")),
                "stable file must be present in coherent changeset: {change_set:?}"
            );
            assert_eq!(
                std::fs::read_to_string(extracted.join("stable.txt")).unwrap(),
                "stable-before-cancel\n"
            );
            if let Ok(partial) = std::fs::read(extracted.join("partial.log")) {
                assert!(
                    std::str::from_utf8(&partial).is_ok(),
                    "partial workspace log must remain UTF-8 text"
                );
                assert!(
                    !partial.contains(&0),
                    "partial workspace log must not contain NUL bytes"
                );
            }
        }
        Err(m80_firecracker::FcError::Storage(_)) => {}
        Err(other) => panic!(
            "writeback after cancelled exec must be coherent or typed storage error, got {other:?}"
        ),
    }

    stopped.delete().expect("delete");
}

/// Three sequential execs accumulate a numeric counter in /tmp/n.
///
/// exec1 writes "1"; exec2 reads and increments to "2"; exec3 reads and
/// increments to "3".  Verifies three-turn state accumulation.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn three_execs_increment_counter() {
    let (backend, run_root) = make_backend();
    let vm_id = "persist-counter";
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");

    // exec1: initialise counter to 1.
    let r1 = exec_sh(&mut running, "echo 1 > /tmp/n && cat /tmp/n");
    assert_eq!(r1.exit_code, Some(0), "exec1 must succeed");
    let out1 = String::from_utf8_lossy(&r1.stdout);
    assert!(out1.trim() == "1", "exec1 should see 1, got: {out1:?}");

    // exec2: read + increment → 2.
    let r2 = exec_sh(
        &mut running,
        "n=$(cat /tmp/n) && echo $((n + 1)) > /tmp/n && cat /tmp/n",
    );
    assert_eq!(r2.exit_code, Some(0), "exec2 must succeed");
    let out2 = String::from_utf8_lossy(&r2.stdout);
    assert!(out2.trim() == "2", "exec2 should see 2, got: {out2:?}");

    // exec3: read + increment → 3.
    let r3 = exec_sh(
        &mut running,
        "n=$(cat /tmp/n) && echo $((n + 1)) > /tmp/n && cat /tmp/n",
    );
    assert_eq!(r3.exit_code, Some(0), "exec3 must succeed");
    let out3 = String::from_utf8_lossy(&r3.stdout);
    assert!(out3.trim() == "3", "exec3 should see 3, got: {out3:?}");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}

/// A nonzero exit code from one exec does not poison the vsock channel.
///
/// exec1 runs `/bin/false` (exits 1); exec2 runs `echo ok-after-fail` on
/// the same VM and must succeed.
#[test]
#[ignore = "requires KVM host with real Firecracker binary"]
fn exec_after_failed_exec_still_works() {
    let (backend, run_root) = make_backend();
    let vm_id = "persist-after-fail";
    let _dump_guard = RunDirDumpGuard::new(run_root.join(vm_id));

    let sandbox = backend.admit(sandbox_config(vm_id)).expect("admit");
    let mut running = sandbox.launch().expect("launch");

    // exec1: deliberate failure.
    let r1 = running
        .exec(m80_proto::ExecRequest {
            program: "/bin/false".into(),
            args: vec![],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        })
        .expect("exec must not return Err for nonzero exit");
    assert_ne!(r1.exit_code, Some(0), "exec1 must exit nonzero");

    // exec2: channel must still be intact.
    let r2 = exec_sh(&mut running, "echo ok-after-fail");
    assert_eq!(
        r2.exit_code,
        Some(0),
        "exec2 must succeed after failed exec1"
    );
    let out2 = String::from_utf8_lossy(&r2.stdout);
    assert!(out2.contains("ok-after-fail"), "exec2 stdout: {out2:?}");

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
