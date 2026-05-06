use std::os::unix::net::UnixStream;

use m80_firecracker::{ExecRequest, FcError, WarmPool};

use super::control::{
    self, WarmControlResponse, WarmErrorResponse, WarmRunResult, WarmStreamFrame,
};
use super::status::{self, WarmOwnerIdentity};

pub(super) fn handle_run(
    pool: &WarmPool,
    identity: &WarmOwnerIdentity,
    profile: Option<String>,
    egress: &str,
    request_id: String,
    request: ExecRequest,
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
    let reset_decision = format!("{:?}", lease.reset_decision());
    let discard_reason = format!("{:?}", lease.discard_reason());
    let response = match lease.exec_with_request_id(request, request_id.clone()) {
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
        response,
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
    let reset_decision = format!("{:?}", lease.reset_decision());
    let discard_reason = format!("{:?}", lease.discard_reason());
    let exit = lease.exec_streaming_with_request_id(request, request_id.clone(), |chunk| {
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
        exit,
        reset_decision,
        discard_reason,
        run_dir,
    };
    let _ = control::write_stream_frame(stream, &frame);
}

pub(super) struct StreamingRun {
    pub(super) profile: Option<String>,
    pub(super) egress: String,
    pub(super) request_id: String,
    pub(super) request: ExecRequest,
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
        return Err(FcError::Config(format!(
            "warm profile mismatch: requested {}, owner active {}",
            requested, identity.profile
        )));
    }
    if egress != identity.egress {
        return Err(FcError::Config(format!(
            "warm egress mismatch: requested {}, owner active {}",
            egress, identity.egress
        )));
    }
    if !accepting_leases {
        return Err(FcError::Config(
            "warm owner is draining and not accepting leases".to_owned(),
        ));
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
    use std::sync::Arc;

    use m80_firecracker::{
        Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig, SnapshotPaths,
        WarmPoolConfig,
    };

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
            ready_probe(),
            true,
        );

        match response {
            WarmControlResponse::Error(err) => {
                assert_eq!(err.variant, "PoolEmpty");
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
            Backend::new(BackendConfig {
                discovery,
                max_concurrent_vms: 1,
                run_root: dir.path().to_path_buf(),
                jail_uid: 3000,
                jail_gid: 3000,
                cgroup_mode: CgroupMode::Disabled,
            })
            .expect("backend"),
        );
        WarmPool::new(
            backend,
            WarmPoolConfig {
                target_ready: 1,
                snapshot: SnapshotPaths {
                    vm_state: dir.path().join("vm.snap"),
                    mem: dir.path().join("mem.snap"),
                },
                sandbox: sandbox_config("fixture"),
                ready_probe: ready_probe(),
                vm_id_prefix: "fixture".to_owned(),
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
            boot_args: None,
            overlay_size_bytes: 512 * 1024 * 1024,
            idle_timeout: None,
            request_id: None,
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
        m80_preflight::Discovery {
            firecracker_bin: "/tmp/firecracker".into(),
            jailer_bin: "/tmp/jailer".into(),
            kernel: "/tmp/vmlinux".into(),
            rootfs: "/tmp/rootfs.ext4".into(),
            manifest: fake_manifest(),
            run_root: run_root.into(),
            privilege: m80_preflight::PrivilegeStatus::Root,
            report: Vec::new(),
        }
    }

    fn fake_manifest() -> m80_image_manifest::Manifest {
        m80_image_manifest::Manifest {
            boot_target: None,
            daemon_binary_path: "/tmp/m80-guestd".into(),
            daemon_binary_sha256: "0".repeat(64),
            expected_firecracker_version: "v1.0.0".to_owned(),
            guest_port: 52,
            image_kind: m80_image_manifest::ImageKind::Minimal,
            kernel_image: "/tmp/vmlinux".into(),
            kernel_image_sha256: "1".repeat(64),
            kernel_kind: m80_image_manifest::KernelKind::Stock,
            no_egress_reason: None,
            output_rootfs_image: "/tmp/rootfs.ext4".into(),
            output_rootfs_sha256: "2".repeat(64),
            ready_marker: "M80_READY".to_owned(),
            schema_version: m80_image_manifest::SCHEMA_VERSION,
            service_unit_path: None,
            service_unit_sha256: None,
            source_rootfs_image: None,
            source_rootfs_sha256: None,
            workspace_mount_path: None,
            workspace_mount_sha256: None,
        }
    }
}
