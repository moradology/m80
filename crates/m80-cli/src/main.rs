//! `m80` binary entry point.
//!
//! The binary edge only parses argv, delegates to `m80_cli::runner`, and exits
//! with the returned code. Command behavior lives in the library so tests can
//! exercise dispatch without spawning a subprocess.

use clap::Parser;

use m80_cli::args::Cli;

fn main() {
    let cli = Cli::parse();
    let exit_code = match m80_cli::runner::run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(exit_code);
}
