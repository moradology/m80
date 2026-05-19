//! Binary entry point for the jailer hardening wrapper.

use std::ffi::OsStr;

fn main() -> anyhow::Result<()> {
    let raw_args: Vec<_> = std::env::args_os().skip(1).collect();
    if raw_args.len() == 1 && raw_args[0] == OsStr::new("--version") {
        println!("m80-jailer-harden {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let args = m80_jailer_harden::parse_args(raw_args)?;
    m80_jailer_harden::apply_process_hardening(
        args.resource_limits(),
        args.new_cgroup_ns(),
        args.new_net_ns(),
    )?;
    m80_jailer_harden::exec_jailer(args)?;
    Ok(())
}
