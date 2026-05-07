//! Binary entry point for the jailer hardening wrapper.

fn main() -> anyhow::Result<()> {
    m80_jailer_harden::run(std::env::args_os().skip(1))?;
    Ok(())
}
