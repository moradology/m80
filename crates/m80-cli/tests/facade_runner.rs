//! Runner tests: exercise CLI dispatch without spawning the `m80` binary.

use clap::Parser;
use m80_cli::{runner, Cli};

const EXIT_CONFIG: i32 = 6;

#[test]
fn runner_dispatches_version_without_subprocess() {
    let cli = Cli::try_parse_from(["m80", "version"]).unwrap();
    let code = runner::run(cli).unwrap();
    assert_eq!(code, 0);
}


#[test]
fn runner_dispatches_run_tty_invalid_combo_without_backend_work() {
    let cli = Cli::try_parse_from(["m80", "--json", "run", "--tty", "--", "bash"]).unwrap();
    let code = runner::run(cli).unwrap();
    assert_eq!(code, EXIT_CONFIG);
}
