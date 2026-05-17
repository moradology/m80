use std::os::unix::net::UnixStream;

use m80_firecracker::{FcError, WarmPool};

use super::control::{
    self, WarmControlResponse, WarmErrorResponse, WarmRunResult, WarmStreamFrame,
};
use super::status::{self, WarmOwnerIdentity};
use crate::cmds::proto_json::ExecRequestJson;

pub(super) fn handle_run(
    pool: &WarmPool,
    identity: &WarmOwnerIdentity,
    profile: Option<String>,
    egress: &str,
    request_id: String,
    request: ExecRequestJson,
    accepting_leases: bool,
) -> WarmControlResponse {
    if let Err(e) = validate_run_compatibility(identity, profile, egress, accepting_leases) {
        return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
            &e,
            Some(request_id),
        ));
    }

    let mut lease = match pool.try_lease() {
        Ok(lease) => lease,
        Err(e) => {
            return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
                &e,
                Some(request_id),
            ))
        }
    };
    let run_dir = lease.run_dir().display().to_string();
    // BlankVmReset evidence is not wired in v0.1; leases always discard.
    let reset_decision = "Discard".to_owned();
    let discard_reason = "ResetEvidenceUnavailable".to_owned();
    let response = match lease.exec_with_request_id(request.into(), request_id.clone()) {
        Ok(response) => response,
        Err(e) => {
            let _ = lease.discard();
            return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
                &e,
                Some(request_id),
            ));
        }
    };
    if let Err(e) = lease.discard() {
        return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
            &e,
            Some(request_id),
        ));
    }
    WarmControlResponse::Run(WarmRunResult {
        request_id,
        response: response.into(),
        reset_decision,
        discard_reason,
        run_dir,
    })
}

pub(super) fn handle_run_streaming(
    pool: &WarmPool,
    identity: &WarmOwnerIdentity,
    args: StreamingRun,
    stream: &mut UnixStream,
) {
    let StreamingRun {
        profile,
        egress,
        request_id,
        request,
        accepting_leases,
    } = args;

    if let Err(e) = validate_run_compatibility(identity, profile, &egress, accepting_leases) {
        write_error(stream, &e, Some(request_id));
        return;
    }

    let mut lease = match pool.try_lease() {
        Ok(lease) => lease,
        Err(e) => {
            write_error(stream, &e, Some(request_id));
            return;
        }
    };
    let run_dir = lease.run_dir().display().to_string();
    // BlankVmReset evidence is not wired in v0.1; leases always discard.
    let reset_decision = "Discard".to_owned();
    let discard_reason = "ResetEvidenceUnavailable".to_owned();
    let exit = lease.exec_streaming_with_request_id(request.into(), request_id.clone(), |chunk| {
        control::write_stream_frame(stream, &control::stream_frame_for_chunk(chunk))
    });
    let exit = match exit {
        Ok(exit) => exit,
        Err(e) => {
            let _ = lease.discard();
            write_error(stream, &e, Some(request_id));
            return;
        }
    };
    if let Err(e) = lease.discard() {
        write_error(stream, &e, Some(request_id));
        return;
    }
    let frame = WarmStreamFrame::Exit {
        request_id,
        exit: exit.into(),
        reset_decision,
        discard_reason,
        run_dir,
    };
    if let Err(e) = control::write_stream_frame(stream, &frame) {
        eprintln!("warning: failed to write exit frame to streaming caller: {e}");
    }
}

pub(super) struct StreamingRun {
    pub(super) profile: Option<String>,
    pub(super) egress: String,
    pub(super) request_id: String,
    pub(super) request: ExecRequestJson,
    pub(super) accepting_leases: bool,
}

fn validate_run_compatibility(
    identity: &WarmOwnerIdentity,
    profile: Option<String>,
    egress: &str,
    accepting_leases: bool,
) -> Result<(), FcError> {
    let requested = status::requested_profile(profile);
    if requested != identity.profile {
        return Err(FcError::WarmCompatibilityMismatch {
            field: "profile",
            requested,
            active: identity.profile.clone(),
        });
    }
    if egress != identity.egress {
        return Err(FcError::WarmCompatibilityMismatch {
            field: "egress",
            requested: egress.to_owned(),
            active: identity.egress.clone(),
        });
    }
    if !accepting_leases {
        return Err(FcError::WarmOwnerNotAcceptingLeases);
    }
    Ok(())
}

fn write_error(stream: &mut UnixStream, err: &FcError, request_id: Option<String>) {
    let frame = WarmStreamFrame::Error(WarmErrorResponse::from_error_with_request_id(
        err, request_id,
    ));
    let _ = control::write_stream_frame(stream, &frame);
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::Arc;

    use m80_firecracker::{
        Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig, SnapshotPaths,
        WarmPoolConfig, WarmStrategy,
    };
    use m80_proto::ExecRequest;

    use super::*;

    #[test]
    fn run_request_mismatched_profile_fails_before_lease() {
        let identity = identity("minimal", "outbound");
        let err =
            validate_run_compatibility(&identity, Some("ubuntu".to_owned()), "outbound", true)
                .unwrap_err();

        assert!(err.to_string().contains("warm profile mismatch"));
    }

    #[test]
    fn run_request_empty_pool_returns_pool_empty_without_cold_boot() {
        let response = handle_run(
            &empty_pool_fixture(),
            &identity("minimal", "none"),
            Some("minimal".to_owned()),
            "none",
            "req-empty-pool".to_owned(),
            ready_probe().into(),
            true,
        );

        match response {
            WarmControlResponse::Error(err) => {
                assert_eq!(err.variant, super::control::WarmErrorKind::PoolEmpty);
                assert_eq!(err.request_id.as_deref(), Some("req-empty-pool"));
                assert_eq!(err.target_ready, Some(1));
            }
            other => panic!("expected PoolEmpty error, got {other:?}"),
        }
    }

    fn identity(profile: &str, egress: &str) -> WarmOwnerIdentity {
        WarmOwnerIdentity {
            binary_version: "0.0.0".to_owned(),
            profile: profile.to_owned(),
            egress: egress.to_owned(),
            target_ready: 1,
            pid: 1,
            mode: "foreground".to_owned(),
            socket_path: "/tmp/m80.sock".to_owned(),
            started_at_unix_ms: 0,
        }
    }

    fn empty_pool_fixture() -> WarmPool {
        let dir = tempfile::tempdir().unwrap();
        let discovery = fake_discovery(dir.path());
        let backend = Arc::new(
            Backend::new(
                BackendConfig::builder(discovery)
                    .max_concurrent_vms(1)
                    .run_root(dir.path().to_path_buf())
                    .jail_uid(3000)
                    .jail_gid(3000)
                    .cgroup_mode(CgroupMode::Disabled)
                    .build(),
            )
            .expect("backend"),
        );
        WarmPool::new(
            backend,
            WarmPoolConfig {
                target_ready: 1,
                sandbox: sandbox_config("fixture"),
                strategy: WarmStrategy::direct_snapshot(
                    SnapshotPaths {
                        vm_state: dir.path().join("vm.snap"),
                        mem: dir.path().join("mem.snap"),
                    },
                    ready_probe(),
                ),
                vm_id_prefix: "fixture".to_owned(),
                cpu_allocator: None,
            },
        )
        .expect("warm pool")
    }

    fn sandbox_config(vm_id: impl Into<String>) -> SandboxConfig {
        SandboxConfig {
            vm_id: Some(vm_id.into()),
            workspace: None,
            network: NetworkPolicy::NoEgress,
            vcpu_count: None,
            mem_size_mib: None,
            cpuset_cpus: None,
            cpu_template: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        }
    }

    fn ready_probe() -> ExecRequest {
        ExecRequest {
            program: "/bin/true".to_owned(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        }
    }

    fn fake_discovery(run_root: &std::path::Path) -> m80_preflight::Discovery {
        let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
        let rootfs_path = rootfs.path().to_path_buf();
        let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
        let net_helper_bin = fake_net_helper(run_root);
        m80_preflight::Discovery {
            firecracker_bin: "/tmp/firecracker".into(),
            firecracker_seccomp_filter: "/tmp/firecracker-seccomp-filter.bin".into(),
            jailer_bin: "/tmp/jailer".into(),
            jailer_harden_bin: "/tmp/m80-jailer-harden".into(),
            net_helper_bin,
            kernel: "/tmp/vmlinux".into(),
            rootfs: "/tmp/rootfs.ext4".into(),
            pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
            manifest: fake_manifest(),
            run_root: run_root.to_path_buf(),
            privilege: m80_preflight::PrivilegeStatus::Root,
            report: Vec::new(),
        }
    }

    fn fake_net_helper(run_root: &std::path::Path) -> std::path::PathBuf {
        std::fs::create_dir_all(run_root).expect("run root");
        let path = run_root.join("m80-net-helper");
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").expect("fake net helper");
        let mut perms = std::fs::metadata(&path)
            .expect("fake net helper metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("fake net helper executable");
        path
    }

    fn fake_manifest() -> m80_image_manifest::Manifest {
        m80_image_manifest::Manifest::new(
            "/tmp/m80-guestd".into(),
            "0".repeat(64),
            "v1.0.0".to_owned(),
            52,
            m80_image_manifest::ImageKind::Minimal,
            "/tmp/vmlinux".into(),
            "1".repeat(64),
            m80_image_manifest::KernelKind::Stock,
            None,
            "/tmp/rootfs.ext4".into(),
            "2".repeat(64),
            "M80_READY".to_owned(),
            m80_image_manifest::RootfsFormat::Ext4,
            None,
            None,
        )
    }
}
