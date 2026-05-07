use std::fs;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    ExecRequest, FcError, NetworkPolicy, SandboxConfig, SnapshotPaths, WarmPool, WarmPoolConfig,
    WarmPoolSnapshot,
};

use crate::args::EgressMode;
use crate::errors;
use crate::json;

use super::control::{self, WarmControlRequest, WarmControlResponse, WarmErrorResponse};
use super::run;
use super::status::{self, WarmOwnerIdentity};

pub(super) fn run_foreground(
    profile: Option<String>,
    egress: EgressMode,
    size: usize,
    json_mode: bool,
) -> i32 {
    match run_foreground_inner(profile, egress, size, json_mode) {
        Ok(()) => 0,
        Err(e) => errors::render_error(&e, json_mode),
    }
}

fn run_foreground_inner(
    profile: Option<String>,
    egress: EgressMode,
    size: usize,
    json_mode: bool,
) -> Result<(), FcError> {
    let socket_path = status::socket_path()?;
    let warm_root = status::warm_root()?;
    if socket_path.exists() {
        return Err(FcError::config_other(format!(
            "warm owner socket already exists at {}; run `m80 warm disable` first",
            socket_path.display()
        )));
    }
    fs::create_dir_all(&warm_root).map_err(FcError::Io)?;

    let (backend, _effective) = super::super::build_run_backend(profile.clone())?;
    let snapshot_dir = status::snapshot_dir()?;
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir).map_err(FcError::Io)?;
    }
    fs::create_dir_all(&snapshot_dir).map_err(FcError::Io)?;

    let snapshot = SnapshotPaths {
        vm_state: snapshot_dir.join("vm.snap"),
        mem: snapshot_dir.join("mem.snap"),
    };
    let sandbox = warm_sandbox_config("warm-golden", egress);
    let golden = backend.admit(sandbox)?;
    let mut running = golden.launch()?;
    running.capture(snapshot.clone())?;
    running.force_kill()?.delete()?;

    let pool = WarmPool::new(
        Arc::clone(&backend),
        WarmPoolConfig {
            target_ready: size,
            snapshot,
            sandbox: warm_sandbox_config("warm-template", egress),
            ready_probe: ready_probe(),
            vm_id_prefix: "warm-slot".to_owned(),
        },
    )?;
    pool.fill_to_target_blocking()?;

    let listener = UnixListener::bind(&socket_path).map_err(FcError::Io)?;
    let identity = WarmOwnerIdentity {
        binary_version: env!("CARGO_PKG_VERSION").to_owned(),
        profile: status::requested_profile(profile),
        egress: status::egress_label(egress).to_owned(),
        target_ready: size,
        pid: std::process::id(),
        mode: "foreground".to_owned(),
        socket_path: socket_path.display().to_string(),
        started_at_unix_ms: unix_ms_now(),
    };
    status::write_identity(&identity)?;

    let initial = status::available(&identity, None, pool.snapshot(), true, false, None);
    if json_mode {
        println!("{}", json::to_pretty(&initial));
    } else {
        eprintln!(
            "m80 warm owner ready: pid={} socket={} ready={}/{}",
            identity.pid, identity.socket_path, initial.slots.ready, initial.slots.target_ready
        );
    }

    let mut accepting_leases = true;
    let mut draining = false;
    let mut shutdown = false;

    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(FcError::Io)?;
        let response = match control::read_request(&mut stream) {
            Ok(WarmControlRequest::Status { profile }) => {
                WarmControlResponse::Status(status::available(
                    &identity,
                    profile,
                    pool.snapshot(),
                    accepting_leases,
                    draining,
                    None,
                ))
            }
            Ok(WarmControlRequest::Run {
                profile,
                egress,
                request_id,
                request,
            }) => run::handle_run(
                &pool,
                &identity,
                profile,
                &egress,
                request_id,
                request,
                accepting_leases,
            ),
            Ok(WarmControlRequest::RunStream {
                profile,
                egress,
                request_id,
                request,
            }) => {
                run::handle_run_streaming(
                    &pool,
                    &identity,
                    run::StreamingRun {
                        profile,
                        egress,
                        request_id,
                        request,
                        accepting_leases,
                    },
                    &mut stream,
                );
                continue;
            }
            Ok(WarmControlRequest::Drain) => {
                accepting_leases = false;
                draining = true;
                shutdown = true;
                match wait_for_filling_to_settle(&pool, Duration::from_secs(30)) {
                    Ok(()) => WarmControlResponse::Status(status::available(
                        &identity,
                        None,
                        drained_snapshot(&pool),
                        false,
                        true,
                        None,
                    )),
                    Err(e) => WarmControlResponse::Error(WarmErrorResponse::from_error(&e)),
                }
            }
            Ok(WarmControlRequest::Disable) => {
                accepting_leases = false;
                draining = true;
                shutdown = true;
                match wait_for_filling_to_settle(&pool, Duration::from_secs(30)) {
                    Ok(()) => WarmControlResponse::Status(status::disabled(Some(
                        identity.profile.clone(),
                    ))),
                    Err(e) => WarmControlResponse::Error(WarmErrorResponse::from_error(&e)),
                }
            }
            Err(e) => WarmControlResponse::Error(WarmErrorResponse::from_error(&e)),
        };
        control::write_response(&mut stream, &response)?;
        if shutdown {
            break;
        }
    }

    drop(pool);
    status::remove_owner_state()?;
    let _ = fs::remove_dir_all(status::snapshot_dir()?);
    Ok(())
}

fn drained_snapshot(pool: &WarmPool) -> WarmPoolSnapshot {
    let mut snapshot = pool.snapshot();
    snapshot.ready = 0;
    snapshot.filling = 0;
    snapshot
}

fn wait_for_filling_to_settle(pool: &WarmPool, timeout: Duration) -> Result<(), FcError> {
    let deadline = Instant::now() + timeout;
    while pool.snapshot().filling > 0 {
        if Instant::now() >= deadline {
            return Err(FcError::config_other(
                "warm owner drain timed out waiting for filling slots".to_owned(),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn warm_sandbox_config(vm_id: impl Into<String>, egress: EgressMode) -> SandboxConfig {
    SandboxConfig {
        vm_id: Some(vm_id.into()),
        workspace: None,
        network: match egress {
            EgressMode::None => NetworkPolicy::NoEgress,
            EgressMode::Outbound => NetworkPolicy::AllowOutbound { exceptions: vec![] },
        },
        vcpu_count: None,
        mem_size_mib: None,
        boot_args: None,
        overlay_size_bytes: 512 * 1024 * 1024,
        idle_timeout: None,
        daemonize: false,
        request_id: None,
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

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_sandbox_config_is_stateless_and_uses_requested_egress() {
        let none = warm_sandbox_config("warm-none", EgressMode::None);
        assert!(none.workspace.is_none());
        assert_eq!(none.network, NetworkPolicy::NoEgress);
        assert!(none.idle_timeout.is_none());

        let outbound = warm_sandbox_config("warm-outbound", EgressMode::Outbound);
        assert_eq!(
            outbound.network,
            NetworkPolicy::AllowOutbound { exceptions: vec![] }
        );
    }

    #[test]
    fn ready_probe_is_bounded() {
        let req = ready_probe();

        assert_eq!(req.program, "/bin/true");
        assert_eq!(req.timeout_ms, Some(5_000));
    }
}
