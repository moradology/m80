//! `m80-guestd` — in-VM daemon. See `README.md` for the contract.
//! Behavior captures: bead epic `m80-eb8`.

use std::io::{BufReader, BufWriter, Write};
use std::os::fd::AsFd;

use anyhow::Context as _;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use vsock::{VsockListener, VsockStream, VMADDR_CID_ANY, VMADDR_CID_HOST};

mod connection;
mod guest_log;
mod pid_one;

use guest_log::{BootTimer, GuestLogPhase};

/// Parsed command-line arguments.
#[derive(Debug)]
pub(crate) struct Args {
    /// Override the default vsock port (testing only).
    pub(crate) port: Option<u32>,
    /// Print version and exit.
    pub(crate) print_version: bool,
}

/// Hand-rolled argv parser. Walks `std::env::args()` without pulling in clap.
pub(crate) fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        port: None,
        print_version: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" => {
                let val = iter.next().context("--port requires a value")?;
                let n: u32 = val
                    .parse()
                    .with_context(|| format!("invalid --port: {val}"))?;
                args.port = Some(n);
            }
            "--version" => {
                args.print_version = true;
            }
            other => anyhow::bail!("unknown arg: {}", other),
        }
    }
    Ok(args)
}

/// Main run loop. Prints version and returns, or binds vsock and serves.
pub(crate) fn run(args: Args) -> anyhow::Result<()> {
    let mut boot_timer = BootTimer::start();
    boot_timer.mark("process_start");

    if args.print_version {
        println!(
            "m80-guestd {} (proto v{})",
            env!("CARGO_PKG_VERSION"),
            m80_proto::PROTOCOL_VERSION
        );
        boot_timer.mark("version_printed");
        return Ok(());
    }

    let pid_one_mode = pid_one::is_pid_one();
    if pid_one_mode {
        pid_one::enter_pid_one_mode(&mut boot_timer).context("PID-1 setup failed")?;
    } else {
        boot_timer.mark("non_pid1_setup_skipped");
    }
    guest_log::info(GuestLogPhase::Boot, None, "m80-guestd starting");
    boot_timer.mark("guestd_starting_log");

    let port = args.port.unwrap_or(m80_proto::GUEST_PORT_DEFAULT);
    let listener = VsockListener::bind_with_cid_port(VMADDR_CID_ANY, port)
        .with_context(|| format!("failed to bind vsock listener on port {port}"))?;
    boot_timer.mark("exec_listener_bound");
    guest_log::info(
        GuestLogPhase::Ready,
        None,
        format!(
            "{} vsock listener bound on port {port}",
            m80_proto::READY_MARKER_DEFAULT
        ),
    );

    // Signal readiness to the host via inverted-readiness vsock connect.
    // The host pre-created a UnixListener at <vsock_uds>_<READY_PORT_DEFAULT>;
    // Firecracker's muxer routes our outbound connect there. The host's
    // accept() returns event-driven; one byte (PROTOCOL_VERSION) acts as a
    // version handshake and a "guestd is here" signal in one step.
    let mut ready =
        VsockStream::connect_with_cid_port(VMADDR_CID_HOST, m80_proto::READY_PORT_DEFAULT)
            .with_context(|| {
                format!(
                    "failed to connect ready signal to host CID {VMADDR_CID_HOST} port {}",
                    m80_proto::READY_PORT_DEFAULT
                )
            })?;
    ready
        .write_all(&[m80_proto::PROTOCOL_VERSION as u8])
        .context("failed to write proto version on ready signal")?;
    ready.flush().context("failed to flush ready signal")?;
    drop(ready);
    boot_timer.mark("ready_signal_sent");
    guest_log::info(GuestLogPhase::Ready, None, "ready signal sent to host");

    loop {
        let (stream, _addr) = listener.accept().context("vsock accept failed")?;
        // v0.1: sequential — process one connection fully before accepting next.
        let reader = BufReader::new(&stream);
        let writer = BufWriter::new(&stream);
        match connection::handle_connection_with_reader_ready(reader, writer, reader_has_data) {
            Ok(connection::ConnectionOutcome::Continue) => {}
            Ok(connection::ConnectionOutcome::Shutdown(action)) => {
                // The ack has already been flushed back to the host. Drop
                // the stream so the host sees a clean close, then take the
                // termination action that tells the kernel/init system to
                // release Firecracker.
                drop(stream);
                shutdown_terminate(action);
            }
            Err(e) => {
                guest_log::warn(
                    GuestLogPhase::Exec,
                    None,
                    format!("connection handler returned error: {e:#}"),
                );
            }
        }
        if pid_one_mode {
            pid_one::reap_pending();
        }
    }
}

fn reader_has_data(reader: &mut BufReader<&VsockStream>) -> bool {
    if !reader.buffer().is_empty() {
        return true;
    }
    let mut fds = [PollFd::new(reader.get_ref().as_fd(), PollFlags::POLLIN)];
    match poll(&mut fds, PollTimeout::ZERO) {
        Ok(ready) => ready > 0 && fds[0].any().unwrap_or(false),
        Err(e) => {
            guest_log::warn(
                GuestLogPhase::Exec,
                None,
                format!("poll failed while checking cancel readability: {e}"),
            );
            false
        }
    }
}

/// Take the termination action requested via [`ShutdownAction`].
///
/// `Exit`: just `process::exit(0)`. As PID 1 this triggers a kernel panic;
/// with `panic=1` in boot args the kernel reboots and Firecracker exits.
///
/// `Poweroff`: best-effort `/sbin/poweroff -f`, falling back to `exit(0)` if
/// the binary is missing or fails. Used when m80-guestd runs as a systemd
/// service (ubuntu image kind) — exiting alone wouldn't shut the VM down.
fn shutdown_terminate(action: m80_proto::ShutdownAction) -> ! {
    match action {
        m80_proto::ShutdownAction::Exit => std::process::exit(0),
        m80_proto::ShutdownAction::Poweroff => {
            let _ = std::process::Command::new("/sbin/poweroff")
                .arg("-f")
                .status();
            std::process::exit(0);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    run(args)
}
