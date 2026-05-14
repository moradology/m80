//! Binary entry point for the jailer hardening wrapper.

fn main() -> anyhow::Result<()> {
    let args = m80_jailer_harden::parse_args(std::env::args_os().skip(1))?;
    m80_jailer_harden::apply_process_hardening(
        args.resource_limits(),
        args.new_cgroup_ns(),
        args.new_net_ns(),
    )?;
    m80_jailer_harden::exec_jailer(args)?;
    Ok(())
}
