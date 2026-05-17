use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use m80_firecracker::{
    FcError, NetworkPolicy, SandboxConfig, SnapshotPaths, WarmPool, WarmPoolConfig,
    WarmPoolSnapshot, WarmStrategy,
};
use m80_proto::ExecRequest;
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};

use crate::args::EgressMode;
use crate::errors;
use crate::json;

use super::control::{self, WarmControlRequest, WarmControlResponse, WarmErrorResponse};
use super::run;
use super::status::{self, WarmOwnerIdentity};

const OWNER_SOCKET_MODE: u32 = 0o600;
const OWNER_REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
const WARM_SNAPSHOT_FILE_MODE: u32 = 0o444;

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
    fs::create_dir_all(&warm_root).map_err(|source| FcError::PathIo {
        path: warm_root.clone(),
        source,
    })?;
    let identity_path = status::identity_path()?;

    let (backend, _effective) = super::super::build_run_backend(profile.clone())?;
    let snapshot_dir = status::snapshot_dir()?;
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir).map_err(|source| FcError::PathIo {
            path: snapshot_dir.clone(),
            source,
        })?;
    }
    fs::create_dir_all(&snapshot_dir).map_err(|source| FcError::PathIo {
        path: snapshot_dir.clone(),
        source,
    })?;

    let snapshot = SnapshotPaths {
        vm_state: snapshot_dir.join("vm.snap"),
        mem: snapshot_dir.join("mem.snap"),
    };
    let sandbox = warm_sandbox_config("warm-golden", egress);
    let golden = backend.admit(sandbox)?;
    let mut running = golden.launch()?;
    running.capture(snapshot.clone())?;
    let lock_result = lock_warm_snapshot_files(&snapshot);
    let stop_result = running.force_kill().and_then(|stopped| stopped.delete());
    lock_result?;
    stop_result?;

    let pool = WarmPool::new(
        Arc::clone(&backend),
        WarmPoolConfig {
            target_ready: size,
            sandbox: warm_sandbox_config("warm-template", egress),
            strategy: WarmStrategy::direct_snapshot(snapshot, ready_probe()),
            vm_id_prefix: "warm-slot".to_owned(),
            cpu_allocator: None,
        },
    )?;
    pool.fill_to_target_blocking()?;

    let listener = UnixListener::bind(&socket_path).map_err(|source| FcError::PathIo {
        path: socket_path.clone(),
        source,
    })?;
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
        let mut stream =
            incoming.map_err(|source| errors::host_io("accept warm owner connection", source))?;
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
    fs::set_permissions(socket_path, fs::Permissions::from_mode(OWNER_SOCKET_MODE)).map_err(
        |source| FcError::PathIo {
            path: socket_path.to_path_buf(),
            source,
        },
    )
}

fn prepare_owner_stream(stream: &UnixStream) -> Result<(), FcError> {
    authorize_owner_peer(stream)?;
    stream
        .set_read_timeout(Some(OWNER_REQUEST_READ_TIMEOUT))
        .map_err(|source| errors::host_io("set warm owner read timeout", source))
}

fn authorize_owner_peer(stream: &UnixStream) -> Result<(), FcError> {
    let peer = getsockopt(stream, PeerCredentials).map_err(nix_to_io)?;
    let owner_uid = nix::unistd::Uid::effective().as_raw();
    let peer_uid = peer.uid();
    if peer_uid != owner_uid {
        return Err(errors::host_io(
            "authorize warm owner peer",
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("warm owner peer uid {peer_uid} does not match owner uid {owner_uid}"),
            ),
        ));
    }
    Ok(())
}

fn nix_to_io(err: nix::errno::Errno) -> FcError {
    errors::host_io(
        "read warm owner peer credentials",
        io::Error::from_raw_os_error(err as i32),
    )
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

fn lock_warm_snapshot_files(paths: &SnapshotPaths) -> Result<(), FcError> {
    set_readonly_file(&paths.vm_state)?;
    set_readonly_file(&paths.mem)?;
    set_readonly_file(&snapshot_manifest_path(paths)?)?;
    Ok(())
}

fn snapshot_manifest_path(paths: &SnapshotPaths) -> Result<PathBuf, FcError> {
    let Some(parent) = paths.vm_state.parent() else {
        return Err(errors::host_io(
            "resolve warm snapshot manifest parent",
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "snapshot vm_state path must have a parent directory",
            ),
        ));
    };
    Ok(parent.join(m80_snapshot::SNAPSHOT_MANIFEST_FILE))
}

fn set_readonly_file(path: &Path) -> Result<(), FcError> {
    fs::set_permissions(path, fs::Permissions::from_mode(WARM_SNAPSHOT_FILE_MODE)).map_err(
        |source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        },
    )
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
        overlay_clone_mode: Default::default(),
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
    fn lock_warm_snapshot_files_marks_pair_and_manifest_readonly() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let paths = SnapshotPaths {
            vm_state: dir.path().join("vm.snap"),
            mem: dir.path().join("mem.snap"),
        };
        fs::write(&paths.vm_state, b"vm-state").expect("write vm snapshot");
        fs::write(&paths.mem, b"memory").expect("write memory snapshot");
        fs::write(
            dir.path().join(m80_snapshot::SNAPSHOT_MANIFEST_FILE),
            b"manifest",
        )
        .expect("write manifest");

        lock_warm_snapshot_files(&paths).expect("lock warm snapshot");

        assert_eq!(file_mode(&paths.vm_state), WARM_SNAPSHOT_FILE_MODE);
        assert_eq!(file_mode(&paths.mem), WARM_SNAPSHOT_FILE_MODE);
        assert_eq!(
            file_mode(&dir.path().join(m80_snapshot::SNAPSHOT_MANIFEST_FILE)),
            WARM_SNAPSHOT_FILE_MODE
        );
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

    fn file_mode(path: &Path) -> u32 {
        fs::metadata(path).expect("stat path").permissions().mode() & 0o777
    }
}
