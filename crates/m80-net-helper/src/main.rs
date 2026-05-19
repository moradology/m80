//! `m80-net-helper` binary entrypoint.

use std::ffi::OsStr;
use std::io::{self, BufReader};
use std::process::ExitCode;

fn main() -> ExitCode {
    let raw_args: Vec<_> = std::env::args_os().skip(1).collect();
    if raw_args.len() == 1 && raw_args[0] == OsStr::new("--version") {
        println!("m80-net-helper {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    match m80_net_outbound::serve_network_helper_stdio(BufReader::new(stdin.lock()), stdout.lock())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("m80-net-helper: {error}");
            ExitCode::FAILURE
        }
    }
}
