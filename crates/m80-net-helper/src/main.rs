//! `m80-net-helper` binary entrypoint.

use std::io::{self, BufReader};
use std::process::ExitCode;

fn main() -> ExitCode {
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
