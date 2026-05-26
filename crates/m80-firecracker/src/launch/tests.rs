use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use m80_proto::{GUEST_PORT_DEFAULT, READY_PORT_DEFAULT};

use super::ready::accept_ready_signal;
use super::ready::guest_boot_phase_events_from_console_text;
use super::ready::kernel_console_timestamp_range_us;
use super::*;
use crate::WireProtocolError;

const VALID_PMEM_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn write_executable(path: &Path, content: &str) {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

struct PersistentFixtureServer {
    socket_path: PathBuf,
    _dir: tempfile::TempDir,
    handle: std::thread::JoinHandle<Vec<String>>,
}

impl PersistentFixtureServer {
    fn join(self) -> Vec<String> {
        self.handle.join().expect("fixture server panicked")
    }
}

fn spawn_persistent_fixture(responses: Vec<Vec<u8>>) -> PersistentFixtureServer {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("fc.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let handle = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("accept");
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
            requests.push(read_one_http_request(&mut conn));
            conn.write_all(&response).expect("write response");
        }
        requests
    });
    PersistentFixtureServer {
        socket_path,
        _dir: dir,
        handle,
    }
}

fn read_one_http_request(stream: &mut UnixStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let mut expected_total = None;
    loop {
        let n = stream.read(&mut tmp).expect("read request");
        assert!(n > 0, "unexpected EOF while reading request");
        buf.extend_from_slice(&tmp[..n]);
        if let Some(total) = expected_total {
            if buf.len() >= total {
                break;
            }
            continue;
        }
        if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header_len = header_end + 4;
            let header_text = String::from_utf8_lossy(&buf[..header_end]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            let total = header_len + content_length;
            expected_total = Some(total);
            if buf.len() >= total {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn fake_discovery(run_root: &Path) -> m80_preflight::Discovery {
    let rootfs = tempfile::NamedTempFile::new().expect("fake rootfs");
    let rootfs_path = rootfs.path().to_path_buf();
    let rootfs_file = rootfs.reopen().expect("fake rootfs fd");
    let net_helper_bin = fake_net_helper(run_root);
    m80_preflight::Discovery {
        firecracker_bin: PathBuf::from("/tmp/firecracker"),
        firecracker_seccomp_filter: PathBuf::from("/tmp/firecracker-seccomp-filter.bin"),
        jailer_bin: PathBuf::from("/tmp/jailer"),
        firecracker_version: "v1.0.0".to_owned(),
        jailer_version: "v1.0.0".to_owned(),
        jailer_harden_bin: PathBuf::from("/tmp/m80-jailer-harden"),
        net_helper_bin,
        kernel: PathBuf::from("/tmp/vmlinux"),
        rootfs: PathBuf::from("/tmp/rootfs.ext4"),
        pinned_rootfs: m80_preflight::PinnedRootfs::from_file(rootfs_path, rootfs_file),
        manifest: m80_image_manifest::Manifest::new(
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
        ),
        run_root: run_root.to_path_buf(),
        privilege: m80_preflight::PrivilegeStatus::Root,
        report: Vec::new(),
    }
}

fn fake_net_helper(run_root: &Path) -> PathBuf {
    std::fs::create_dir_all(run_root).expect("run root");
    let path = run_root.join("m80-net-helper-test");
    write_executable(
        &path,
        r#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"ok","success":{"kind":"empty"}}'
done
"#,
    );
    path
}

fn fake_backend(run_root: &Path) -> Arc<crate::Backend> {
    let config = crate::BackendConfig::builder(fake_discovery(run_root))
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(crate::CgroupMode::Disabled)
        .build();
    Arc::new(crate::Backend::new(config).expect("Backend::new"))
}

fn pmem_layer(name: &str) -> crate::PmemLayer {
    let digest = crate::ImageDigest::parse(VALID_PMEM_DIGEST).expect("digest");
    let image = crate::ErofsImageRef::from_digest(digest);
    let mount_at =
        crate::GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).expect("mount path");
    crate::PmemLayer::new(image, crate::PmemSharing::PerVm, mount_at)
}

#[test]
fn ready_listener_path_uses_muxer_port_suffix() {
    let vsock = Path::new("/run/m80/vm/firecracker/vm/root/vsock.sock");

    assert_eq!(
        ready_listener_path(vsock),
        PathBuf::from(format!(
            "/run/m80/vm/firecracker/vm/root/vsock.sock_{}",
            READY_PORT_DEFAULT
        ))
    );
}

#[test]
fn phase_1_run_root_prep_creates_owner_only_run_dir() {
    let dir = tempfile::tempdir().unwrap();

    let run_dir = phase_1_run_root_prep(dir.path(), "vm-mode").unwrap();

    let mode = std::fs::metadata(run_dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, RUN_DIR_MODE);
}

#[test]
fn launch_rejects_duplicate_pmem_mounts_before_run_dir_creation() {
    let run_root = tempfile::tempdir().expect("run root");
    let vm_id = "pmem-dupe";
    let backend = fake_backend(run_root.path());
    let config = SandboxConfig {
        vm_id: Some(vm_id.to_owned()),
        pmem_layers: vec![pmem_layer("rust"), pmem_layer("rust")],
        ..SandboxConfig::default()
    };
    let sandbox = backend.admit(config).expect("admit");

    let Err(err) = sandbox.launch() else {
        panic!("launch should reject duplicate pmem mounts")
    };

    assert!(
        matches!(
            err,
            FcError::Config(ConfigError::MountPathDuplicated { .. })
        ),
        "got {err:?}"
    );
    assert!(
        !run_root.path().join(vm_id).exists(),
        "invalid pmem launch must fail before creating the run dir"
    );
}

#[test]
fn phase_1_failure_preserves_existing_run_dir_with_summary() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-phase1");
    std::fs::create_dir(&run_dir).unwrap();
    let err = FcError::InvalidVmId {
        vm_id: "vm-phase1".to_owned(),
        reason: "test failure".to_owned(),
    };

    preserve_launch_failure_artifact_if_run_dir_exists(
        &run_dir,
        "vm-phase1",
        "phase_1_run_root_prep",
        Some("req-phase1"),
        &err,
        false,
    );

    assert!(!run_dir.exists());
    let preserved_parent = dir.path().join(".preserved");
    let preserved: Vec<_> = std::fs::read_dir(&preserved_parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(preserved.len(), 1);

    let summary =
        std::fs::read_to_string(preserved[0].join(crate::diagnostics::FAILURE_SUMMARY_FILE_NAME))
            .unwrap();
    let value: serde_json::Value = serde_json::from_str(&summary).unwrap();
    assert_eq!(value["vm_id"], "vm-phase1");
    assert_eq!(value["failed_phase"], "phase_1_run_root_prep");
    assert_eq!(value["error_variant"], "InvalidVmId");
    assert_eq!(value["request_id"], "req-phase1");
}

#[test]
fn launch_jailer_config_enables_pid_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let jailer_bin = dir.path().join("jailer");
    let jailer_harden_bin = dir.path().join("m80-jailer-harden");
    let firecracker_bin = dir.path().join("firecracker");
    let run_dir = dir.path().join("vm-newpid");

    let config = build_jailer_launch_config(
        JailerLaunchConfigInput {
            jailer_bin: &jailer_bin,
            jailer_harden_bin: &jailer_harden_bin,
            firecracker_bin: &firecracker_bin,
            uid: 3000,
            gid: 3000,
            run_dir: &run_dir,
            daemonize: false,
            netns_path: None,
            private_netns: true,
        },
        Vec::new(),
        vec![JailerSocket::Firecracker, JailerSocket::Vsock],
    );

    assert!(config.new_pid_ns);
    assert!(config.new_net_ns);
    assert_eq!(config.daemonize, false);
    assert_eq!(config.cgroup_version, Some(m80_jailer::CgroupVersion::V2));
    assert_eq!(
        config.seccomp_filter_path.as_deref(),
        Some(Path::new(FIRECRACKER_SECCOMP_FILTER_JAIL_PATH))
    );
}

#[test]
fn allow_outbound_cold_launch_uses_planned_private_netns_path() {
    let dir = tempfile::tempdir().unwrap();
    let run_root = dir.path().join("run-root");
    let vm_id = "vm-outbound";
    let policy = crate::NetworkPolicy::AllowOutbound {
        exceptions: Vec::new(),
    };

    let path = cold_launch_netns_path(&policy, &run_root, vm_id).unwrap();

    assert_eq!(
        path,
        m80_net_outbound::planned_vmm_netns_path(&run_root, vm_id)
    );
}

#[test]
fn phase_6_outbound_nat_realization_uses_network_helper() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("requests.log");
    let helper_path = dir.path().join("helper.sh");
    write_executable(
        &helper_path,
        &format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "{}"
  printf '%s\n' '{{"status":"ok","success":{{"kind":"realized_network","realized":{{"bridge_name":"br-test","tap_name":"tap-test","vmm_netns_path":"/run/netns/m80-test","guest_ipv4":"172.16.0.2","guest_mac":"02:00:00:00:00:01","bridge_cidr":"172.16.0.0/24"}}}}}}'
done
"#,
            log.display()
        ),
    );
    let helper = crate::network_helper::NetworkHelperClient::new(helper_path);
    let run_root = dir.path().join("run-root");
    let run_dir = run_root.join("vm-helper");
    let mut config = SandboxConfig::default();
    config.network = crate::NetworkPolicy::AllowOutbound {
        exceptions: Vec::new(),
    };

    let realized =
        phase_6_network_realize(&helper, &config, "vm-helper", &run_root, &run_dir).unwrap();

    assert!(matches!(
        realized,
        RealizedNetwork::OutboundNat {
            ref tap_name,
            ref guest_mac,
            ..
        } if tap_name == "tap-test" && guest_mac == "02:00:00:00:00:01"
    ));
    let requests = std::fs::read_to_string(log).unwrap();
    assert!(requests.contains(r#""op":"realize_bridge_and_tap""#));
    assert!(requests.contains(r#""vm_id":"vm-helper""#));
}

#[test]
fn phase_6_helper_failure_is_typed() {
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helper.sh");
    write_executable(
        &helper_path,
        r#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"err","failure":{"kind":"operation_failed","detail":"synthetic launch denial"}}'
done
"#,
    );
    let helper = crate::network_helper::NetworkHelperClient::new(helper_path);
    let run_root = dir.path().join("run-root");
    let run_dir = run_root.join("vm-helper-fail");
    let mut config = SandboxConfig::default();
    config.network = crate::NetworkPolicy::AllowOutbound {
        exceptions: Vec::new(),
    };

    let err = phase_6_network_realize(&helper, &config, "vm-helper-fail", &run_root, &run_dir)
        .unwrap_err();

    assert!(matches!(
        err,
        FcError::NetworkHelper(crate::NetworkHelperError::OperationFailed {
            operation: crate::NetworkHelperOperation::RealizeBridgeAndTap,
            kind: m80_net_outbound::NetworkHelperFailureKind::OperationFailed,
            ref detail,
        }) if detail == "synthetic launch denial"
    ));
}

#[test]
fn no_egress_cold_launch_still_uses_hardener_private_netns() {
    let dir = tempfile::tempdir().unwrap();

    assert!(
        cold_launch_netns_path(&crate::NetworkPolicy::NoEgress, dir.path(), "vm-no-egress")
            .is_none()
    );
    assert!(private_vmm_netns(&crate::NetworkPolicy::NoEgress));
}

#[test]
fn snapshot_binding_creates_jail_destination_before_bind() {
    let mut bindings = Vec::new();

    push_snapshot_bindings(
        &mut bindings,
        PathBuf::from("/var/lib/m80-run/snapshots/source"),
        BindMode::Ro,
    );

    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].dest, PathBuf::from(SNAPSHOT_BIND_DEST));
    assert_eq!(bindings[0].mode, BindMode::CreateInsideJail);
    assert_eq!(
        bindings[1].source,
        PathBuf::from("/var/lib/m80-run/snapshots/source")
    );
    assert_eq!(bindings[1].dest, PathBuf::from(SNAPSHOT_BIND_DEST));
    assert_eq!(bindings[1].mode, BindMode::Ro);
}

#[test]
fn pmem_backings_are_bound_ro_to_slot_jail_paths() {
    let mut bindings = Vec::new();
    let backings = vec![crate::types::ResolvedPmemBacking {
        host_path: PathBuf::from("/var/lib/m80-run/vm/pmem/0.img"),
        jail_basename: "pmem.0.img".to_owned(),
        sharing: crate::PmemSharing::PerVm,
    }];

    push_pmem_backing_bindings(&mut bindings, &backings);

    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].source,
        PathBuf::from("/var/lib/m80-run/vm/pmem/0.img")
    );
    assert_eq!(bindings[0].dest, PathBuf::from("pmem.0.img"));
    assert_eq!(bindings[0].mode, BindMode::Ro);
}

#[test]
fn shared_pmem_backings_are_bound_with_image_store_ro_mode() {
    let mut bindings = Vec::new();
    let backings = vec![crate::types::ResolvedPmemBacking {
        host_path: PathBuf::from("/var/lib/m80-images/58/digest/image.erofs"),
        jail_basename: "pmem.0.img".to_owned(),
        sharing: crate::PmemSharing::Shared(crate::TrustDomainAck::new(
            crate::TrustReason::SameOperator,
        )),
    }];

    push_pmem_backing_bindings(&mut bindings, &backings);

    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].source,
        PathBuf::from("/var/lib/m80-images/58/digest/image.erofs")
    );
    assert_eq!(bindings[0].dest, PathBuf::from("pmem.0.img"));
    assert_eq!(bindings[0].mode, BindMode::RoImageStore);
}

#[test]
fn ready_signal_accepts_protocol_version_byte() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();
    let client_path = ready_path.clone();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(client_path).unwrap();
        stream
            .write_all(&[m80_proto::PROTOCOL_VERSION as u8])
            .unwrap();
    });

    accept_ready_signal(&listener, &ready_path, Duration::from_secs(1)).unwrap();

    client.join().unwrap();
}

#[test]
fn ready_signal_wakes_without_fixed_ten_ms_poll_floor() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();
    let client_path = ready_path.clone();
    let delay = Duration::from_millis(41);
    let client = std::thread::spawn(move || {
        std::thread::sleep(delay);
        let mut stream = UnixStream::connect(client_path).unwrap();
        stream
            .write_all(&[m80_proto::PROTOCOL_VERSION as u8])
            .unwrap();
    });

    let started = Instant::now();
    accept_ready_signal(&listener, &ready_path, Duration::from_secs(1)).unwrap();
    let elapsed = started.elapsed();

    client.join().unwrap();
    assert!(
        elapsed < delay + Duration::from_millis(8),
        "ready accept should wake on fd readiness, not the old 10ms poll; elapsed={elapsed:?}"
    );
}

#[test]
fn ready_timeout_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();

    let err = accept_ready_signal(&listener, &ready_path, Duration::from_millis(1)).unwrap_err();

    assert!(
        matches!(err, FcError::GuestdReadyTimeout { ref path, timeout }
            if *path == ready_path && timeout == Duration::from_millis(1)),
        "unexpected error: {err:?}"
    );
}

#[test]
fn ready_signal_rejects_wrong_protocol_version() {
    let dir = tempfile::tempdir().unwrap();
    let ready_path = dir.path().join("ready.sock");
    let listener = UnixListener::bind(&ready_path).unwrap();
    let client_path = ready_path.clone();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(client_path).unwrap();
        stream.write_all(&[0]).unwrap();
    });

    let err = accept_ready_signal(&listener, &ready_path, Duration::from_secs(1)).unwrap_err();

    assert!(
        matches!(
            err,
            FcError::Protocol(WireProtocolError::UnsupportedVersion {
                expected: m80_proto::PROTOCOL_VERSION,
                got: 0
            })
        ),
        "unexpected error: {err:?}"
    );
    client.join().unwrap();
}

#[test]
fn phase_10_open_uds_retries_after_socket_create_wait() {
    let dir = tempfile::tempdir().unwrap();
    let api_socket = dir.path().join("firecracker.sock");
    let mut wait_calls = 0;
    let mut server = None;

    let client = phase_10_open_uds_with_wait(&api_socket, |path, _deadline| {
        wait_calls += 1;
        let listener = UnixListener::bind(path).unwrap();
        server = Some(std::thread::spawn(move || {
            let _conn = listener.accept().unwrap();
        }));
        Ok(())
    })
    .unwrap();

    drop(client);
    server.take().unwrap().join().unwrap();

    assert_eq!(
        wait_calls, 1,
        "phase 10 should wait for socket creation before retrying Client::new"
    );
}

#[test]
fn phase_10b_fc_logger_creates_jail_file_and_puts_logger_config() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-logger");
    let firecracker_bin = PathBuf::from("/usr/bin/firecracker");
    let host_log_path = fc_log_path(&run_dir, &firecracker_bin);
    std::fs::create_dir_all(host_log_path.parent().unwrap()).unwrap();
    let server = m80_test_helpers::fixture_server::SingleFixtureServer::spawn(
        m80_test_helpers::fixture_server::resp_204(),
    )
    .unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    phase_10b_fc_logger(
        &client,
        &run_dir,
        &firecracker_bin,
        nix::unistd::Uid::effective().as_raw(),
        nix::unistd::Gid::effective().as_raw(),
        Some(crate::FcLogLevel::Info),
    )
    .unwrap();

    let metadata = std::fs::metadata(&host_log_path).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let request = server.join().request;
    assert!(request.starts_with("PUT /logger HTTP/1.1\r\n"));
    assert!(request.contains("\"log_path\":\"/firecracker.log\""));
    assert!(request.contains("\"level\":\"Info\""));
    assert!(request.contains("\"show_level\":true"));
    assert!(request.contains("\"show_log_origin\":true"));
}

#[test]
fn phase_10b_fc_logger_truncates_existing_jail_file() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-logger-stale");
    let firecracker_bin = PathBuf::from("/usr/bin/firecracker");
    let host_log_path = fc_log_path(&run_dir, &firecracker_bin);
    std::fs::create_dir_all(host_log_path.parent().unwrap()).unwrap();
    std::fs::write(&host_log_path, b"stale-firecracker-log").unwrap();
    std::fs::set_permissions(&host_log_path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let server = m80_test_helpers::fixture_server::SingleFixtureServer::spawn(
        m80_test_helpers::fixture_server::resp_204(),
    )
    .unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    phase_10b_fc_logger(
        &client,
        &run_dir,
        &firecracker_bin,
        nix::unistd::Uid::effective().as_raw(),
        nix::unistd::Gid::effective().as_raw(),
        None,
    )
    .unwrap();

    assert_eq!(std::fs::read(&host_log_path).unwrap(), b"");
    assert_eq!(
        std::fs::metadata(&host_log_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        server.join().request.contains("\"level\":\"Warning\""),
        "missing default Warning logger level"
    );
}

#[test]
fn phase_10b_fc_metrics_creates_jail_file_and_puts_metrics_config() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-metrics");
    let firecracker_bin = PathBuf::from("/usr/bin/firecracker");
    let host_metrics_path = fc_metrics_path(&run_dir, &firecracker_bin);
    std::fs::create_dir_all(host_metrics_path.parent().unwrap()).unwrap();
    let server = m80_test_helpers::fixture_server::SingleFixtureServer::spawn(
        m80_test_helpers::fixture_server::resp_204(),
    )
    .unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    phase_10b_fc_metrics(
        &client,
        &run_dir,
        &firecracker_bin,
        nix::unistd::Uid::effective().as_raw(),
        nix::unistd::Gid::effective().as_raw(),
    )
    .unwrap();

    let metadata = std::fs::metadata(&host_metrics_path).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let request = server.join().request;
    assert!(request.starts_with("PUT /metrics HTTP/1.1\r\n"));
    assert!(request.contains("\"metrics_path\":\"/firecracker-metrics.jsonl\""));
}

#[test]
fn phase_10b_fc_diagnostics_puts_logger_then_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let run_dir = dir.path().join("vm-diagnostics");
    let firecracker_bin = PathBuf::from("/usr/bin/firecracker");
    let log_path = fc_log_path(&run_dir, &firecracker_bin);
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    let server = spawn_persistent_fixture(vec![
        m80_test_helpers::fixture_server::resp_204(),
        m80_test_helpers::fixture_server::resp_204(),
    ]);
    let client = Client::new(&server.socket_path).unwrap();

    phase_10b_fc_diagnostics(
        &client,
        &run_dir,
        &firecracker_bin,
        nix::unistd::Uid::effective().as_raw(),
        nix::unistd::Gid::effective().as_raw(),
        Some(crate::FcLogLevel::Debug),
    )
    .unwrap();

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("PUT /logger HTTP/1.1\r\n"));
    assert!(requests[0].contains("\"level\":\"Debug\""));
    assert!(requests[1].starts_with("PUT /metrics HTTP/1.1\r\n"));
    assert!(fc_log_path(&run_dir, &firecracker_bin).is_file());
    assert!(fc_metrics_path(&run_dir, &firecracker_bin).is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn wait_for_api_socket_create_returns_after_socket_appears() {
    let dir = tempfile::tempdir().unwrap();
    let api_socket = dir.path().join("firecracker.sock");
    let waiter_path = api_socket.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        wait_for_api_socket_create(&waiter_path, Instant::now() + Duration::from_secs(1))
    });

    ready_rx.recv().unwrap();
    let _listener = UnixListener::bind(api_socket).unwrap();
    waiter.join().unwrap().unwrap();
}

#[test]
fn restore_probe_times_out_when_connected_peer_stalls() {
    let dir = tempfile::tempdir().unwrap();
    let vsock = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&vsock).unwrap();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, format!("CONNECT {GUEST_PORT_DEFAULT}\n"));
        let mut writer = stream;
        writer
            .write_all(format!("OK {GUEST_PORT_DEFAULT}\n").as_bytes())
            .unwrap();
        writer.flush().unwrap();
        std::thread::sleep(Duration::from_millis(100));
    });

    let err = try_restore_exec_probe(
        &vsock,
        "restore-stalled",
        1,
        Instant::now() + Duration::from_millis(20),
    )
    .unwrap_err();

    assert!(
        matches!(err, FcError::GuestdReadyTimeout { .. }),
        "expected GuestdReadyTimeout, got {err:?}"
    );
    handle.join().unwrap();
}

#[test]
fn guest_vsock_port_is_9001() {
    assert_eq!(GUEST_PORT_DEFAULT, 9001);
}

#[test]
fn console_guest_boot_markers_become_phase_12b_events() {
    let events = guest_boot_phase_events_from_console_text(
        "noise\nM80_GUEST_BOOT name=overlayfs_mounted elapsed_us=1234 delta_us=56\n",
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].phase_name, "phase_12b_guest_overlayfs_mounted");
    assert_eq!(events[0].elapsed_us, 1234);
}

#[test]
fn console_guest_boot_parser_ignores_malformed_lines() {
    let events = guest_boot_phase_events_from_console_text(
        "M80_GUEST_BOOT name=missing_elapsed delta_us=5\nM80_GUEST_BOOT elapsed_us=99 delta_us=5\n",
    );

    assert!(events.is_empty());
}

#[test]
fn console_guest_boot_parser_rejects_injected_phase_names() {
    let events = guest_boot_phase_events_from_console_text(
        "M80_GUEST_BOOT name=ok_phase elapsed_us=1\n\
         M80_GUEST_BOOT name=BadPhase elapsed_us=2\n\
         M80_GUEST_BOOT name=evil:admin elapsed_us=3\n",
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].phase_name, "phase_12b_guest_ok_phase");
}

#[test]
fn kernel_console_timestamp_range_uses_first_and_last_stamp() {
    let range = kernel_console_timestamp_range_us(
        "[    0.000000] Linux version x\n[    0.120250] Freeing unused kernel image\nnoise\n",
    );

    assert_eq!(range, Some(120_250));
}

#[test]
fn kernel_console_timestamp_range_accepts_prefixed_lines() {
    let range =
        kernel_console_timestamp_range_us("earlycon: [    1.500000] boot\n[    2.000001] later\n");

    assert_eq!(range, Some(500_001));
}

#[test]
fn proc_io_parser_keeps_kernel_counter_names() {
    let counters = parse_proc_io_text("rchar: 12\nread_bytes: 4096\ncancelled_write_bytes: 0\n");

    assert_eq!(counters["proc_io_rchar"], "12");
    assert_eq!(counters["proc_io_read_bytes"], "4096");
    assert_eq!(counters["proc_io_cancelled_write_bytes"], "0");
}

#[test]
fn proc_stat_major_faults_parser_handles_comm_with_spaces() {
    let stat = "123 (fire cracker) S 1 2 3 4 5 6 7 8 42 10 11";

    assert_eq!(proc_stat_major_faults_from_text(stat), Some(42));
}

#[test]
fn snapshot_file_prime_accepts_existing_snapshot_pair() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SnapshotPaths {
        vm_state: dir.path().join("vm.snap"),
        mem: dir.path().join("mem.snap"),
    };
    std::fs::write(&paths.vm_state, b"vm-state").unwrap();
    std::fs::write(&paths.mem, b"memory").unwrap();

    prime_snapshot_files(&paths).expect("existing snapshot files should prime");
}

#[test]
fn snapshot_file_prime_fails_with_path_for_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = SnapshotPaths {
        vm_state: dir.path().join("vm.snap"),
        mem: dir.path().join("missing-mem.snap"),
    };
    std::fs::write(&paths.vm_state, b"vm-state").unwrap();

    let err = prime_snapshot_files(&paths).unwrap_err();

    assert!(
        matches!(err, FcError::PathIo { ref path, .. } if path == &paths.mem),
        "missing snapshot file should surface as PathIo for mem path, got {err:?}"
    );
}
