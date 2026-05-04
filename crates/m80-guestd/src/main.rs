//! `m80-guestd` — in-VM daemon. See `README.md` for the contract.
//! Behavior captures: bead epic `m80-eb8`.

use std::io::{BufReader, BufWriter};

use anyhow::Context as _;
use vsock::{VsockListener, VMADDR_CID_ANY};

mod connection;

/// Parsed command-line arguments.
#[derive(Debug)]
pub struct Args {
    /// Override the default vsock port (testing only).
    pub port: Option<u32>,
    /// Print version and exit.
    pub print_version: bool,
}

/// Hand-rolled argv parser. Walks `std::env::args()` without pulling in clap.
pub fn parse_args() -> anyhow::Result<Args> {
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
pub fn run(args: Args) -> anyhow::Result<()> {
    if args.print_version {
        println!(
            "m80-guestd {} (proto v{})",
            env!("CARGO_PKG_VERSION"),
            m80_proto::PROTOCOL_VERSION
        );
        return Ok(());
    }

    let port = args.port.unwrap_or(m80_proto::GUEST_PORT_DEFAULT);
    let listener = VsockListener::bind_with_cid_port(VMADDR_CID_ANY, port)
        .with_context(|| format!("failed to bind vsock listener on port {port}"))?;

    // Signal to systemd / the host that we are ready.
    println!("{}", m80_proto::READY_MARKER_DEFAULT);

    loop {
        let (stream, _addr) = listener.accept().context("vsock accept failed")?;
        // v0.1: sequential — process one connection fully before accepting next.
        let reader = BufReader::new(&stream);
        let writer = BufWriter::new(&stream);
        if let Err(e) = connection::handle_connection(reader, writer) {
            tracing::warn!(error = %e, "connection handler returned error");
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    run(args)
}
