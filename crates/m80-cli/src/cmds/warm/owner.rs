use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    FcError, NetworkPolicy, SandboxConfig, SnapshotPaths, WarmPool, WarmPoolConfig,
    WarmPoolSnapshot,
};
use m80_proto::ExecRequest;
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use nix::unistd::Uid;

use crate::args::EgressMode;
use crate::errors;
use crate::json;

use super::control::{self, WarmControlRequest, WarmControlResponse, WarmErrorResponse};
use super::run;
use super::status::{self, WarmOwnerIdentity};

const OWNER_SOCKET_MODE: u32 = 0o600;
const OWNER_REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);

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
        return Err(FcError::WarmOwnerSocketExists { socket_path });
    }
    fs::create_dir_all(&warm_root).map_err(FcError::Io)?;
    let identity_path = status::identity_path()?;

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
            cpu_allocator: None,
        },
    )?;
    pool.fill_to_target_blocking()?;

    let listener = UnixListener::bind(&socket_path).map_err(FcError::Io)?;
    let mut owner_state_guard = WarmOwnerStateGuard::new(socket_path.clone(), identity_path);
    restrict_owner_socket(&socket_path)?;
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
        let request =
            prepare_owner_stream(&stream).and_then(|()| control::read_request(&mut stream));
        let response = match request {
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
    owner_state_guard.disarm();
    let _ = fs::remove_dir_all(status::snapshot_dir()?);
    Ok(())
}

fn restrict_owner_socket(socket_path: &Path) -> Result<(), FcError> {
    fs::set_permissions(socket_path, fs::Permissions::from_mode(OWNER_SOCKET_MODE))
        .map_err(FcError::Io)
}

fn prepare_owner_stream(stream: &UnixStream) -> Result<(), FcError> {
    authorize_owner_peer(stream)?;
    stream
        .set_read_timeout(Some(OWNER_REQUEST_READ_TIMEOUT))
        .map_err(FcError::Io)
}

fn authorize_owner_peer(stream: &UnixStream) -> Result<(), FcError> {
    let peer = getsockopt(stream, PeerCredentials).map_err(nix_to_io)?;
    let owner_uid = Uid::effective().as_raw();
    let peer_uid = peer.uid();
    if peer_uid != owner_uid {
        return Err(FcError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("warm owner peer uid {peer_uid} does not match owner uid {owner_uid}"),
        )));
    }
    Ok(())
}

fn nix_to_io(err: nix::errno::Errno) -> FcError {
    FcError::Io(io::Error::from_raw_os_error(err as i32))
}

#[derive(Debug)]
struct WarmOwnerStateGuard {
    socket_path: PathBuf,
    identity_path: PathBuf,
    armed: bool,
}

impl WarmOwnerStateGuard {
    fn new(socket_path: PathBuf, identity_path: PathBuf) -> Self {
        Self {
            socket_path,
            identity_path,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for WarmOwnerStateGuard {
    fn drop(&mut self) {
        if self.armed {
            remove_file_if_present(&self.socket_path);
            remove_file_if_present(&self.identity_path);
        }
    }
}

fn remove_file_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn drained_snapshot(pool: &WarmPool) -> WarmPoolSnapshot {
    let mut snapshot = pool.snapshot();
    snapshot.ready = 0;
    snapshot.filling = 0;
    snapshot
}

fn wait_for_filling_to_settle(pool: &WarmPool, timeout: Duration) -> Result<(), FcError> {
    pool.wait_for_idle(timeout)
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
        cpuset_cpus: None,
        cpu_template: None,
        drive_cache_type: None,
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
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(ms).unwrap_or(u64::MAX)
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

    #[test]
    fn owner_socket_is_restricted_to_owner_uid() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let socket_path = dir.path().join("owner.sock");
        let _listener = UnixListener::bind(&socket_path).expect("bind socket");

        restrict_owner_socket(&socket_path).expect("restrict socket");

        let mode = fs::metadata(&socket_path)
            .expect("stat socket")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, OWNER_SOCKET_MODE);
    }

    #[test]
    fn same_uid_peer_is_authorized() {
        let (_client, server) = UnixStream::pair().expect("create socket pair");

        authorize_owner_peer(&server).expect("same uid peer is authorized");
    }

    #[test]
    fn owner_state_guard_unlinks_socket_and_identity() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let socket_path = dir.path().join("owner.sock");
        let identity_path = dir.path().join("owner.json");
        fs::write(&socket_path, b"socket placeholder").expect("write socket placeholder");
        fs::write(&identity_path, b"identity").expect("write identity");

        {
            let _guard = WarmOwnerStateGuard::new(socket_path.clone(), identity_path.clone());
        }

        assert!(!socket_path.exists());
        assert!(!identity_path.exists());
    }

    #[test]
    fn disarmed_owner_state_guard_preserves_cleanly_removed_paths() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let socket_path = dir.path().join("owner.sock");
        let identity_path = dir.path().join("owner.json");
        fs::write(&socket_path, b"socket placeholder").expect("write socket placeholder");
        fs::write(&identity_path, b"identity").expect("write identity");

        let mut guard = WarmOwnerStateGuard::new(socket_path.clone(), identity_path.clone());
        fs::remove_file(&socket_path).expect("remove socket");
        fs::remove_file(&identity_path).expect("remove identity");
        guard.disarm();
        drop(guard);

        assert!(!socket_path.exists());
        assert!(!identity_path.exists());
    }
}
