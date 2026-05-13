use std::io::Write;
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
fn phase_10_open_uds_wakes_on_socket_create_event() {
    let dir = tempfile::tempdir().unwrap();
    let api_socket = dir.path().join("firecracker.sock");
    let server_path = api_socket.clone();
    let delay = Duration::from_millis(20);
    let server = std::thread::spawn(move || {
        std::thread::sleep(delay);
        let listener = UnixListener::bind(server_path).unwrap();
        let _conn = listener.accept().unwrap();
    });

    let started = Instant::now();
    let client = phase_10_open_uds(&api_socket).unwrap();
    let elapsed = started.elapsed();
    drop(client);
    server.join().unwrap();

    assert!(
        elapsed < delay + Duration::from_millis(25),
        "phase 10 should wake from the socket create event, not the old 50ms poll; elapsed={elapsed:?}"
    );
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
