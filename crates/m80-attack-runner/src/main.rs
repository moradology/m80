//! CLI entrypoint for the m80 malicious attack runner.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args
        .next()
        .unwrap_or_else(|| "m80-attack-runner".to_owned());
    let Some(attack) = args.next() else {
        eprintln!("usage: {program} <attack-name>");
        eprintln!("known attacks:");
        for name in m80_attack_runner::attack_names() {
            eprintln!("  {name}");
        }
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: {program} <attack-name>");
        return ExitCode::from(2);
    }

    match m80_attack_runner::run_attack(&attack) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}
