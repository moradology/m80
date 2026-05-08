//! Test-only adversarial guest daemon for real-KVM guest-to-host wire tests.

use std::io::Write;

use anyhow::Context as _;
use vsock::{VsockListener, VsockStream, VMADDR_CID_ANY, VMADDR_CID_HOST};

const ATTACK_ENV: &str = "M80_MALICIOUS_ATTACK";
const CMDLINE_KEY: &str = "m80.malicious_attack";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attack {
    Noop,
}

impl Attack {
    fn parse(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "noop" => Ok(Attack::Noop),
            other => anyhow::bail!("unknown malicious guestd attack: {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Attack::Noop => "noop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Args {
    attack: Option<Attack>,
    check_config: bool,
    list_attacks: bool,
    print_version: bool,
}

fn parse_args_from<I>(args: I) -> anyhow::Result<Args>
where
    I: IntoIterator<Item = String>,
{
    let mut parsed = Args {
        attack: None,
        check_config: false,
        list_attacks: false,
        print_version: false,
    };
    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--attack" => {
                let val = iter.next().context("--attack requires a value")?;
                parsed.attack = Some(Attack::parse(&val)?);
            }
            "--check-config" => parsed.check_config = true,
            "--list-attacks" => parsed.list_attacks = true,
            "--version" => parsed.print_version = true,
            other => anyhow::bail!("unknown arg: {other}"),
        }
    }
    Ok(parsed)
}

fn main() -> anyhow::Result<()> {
    let args = parse_args_from(std::env::args())?;
    run(args)
}

fn run(args: Args) -> anyhow::Result<()> {
    if args.print_version {
        println!(
            "m80-guestd-malicious {} (proto v{})",
            env!("CARGO_PKG_VERSION"),
            m80_proto::PROTOCOL_VERSION
        );
        return Ok(());
    }
    if args.list_attacks {
        println!("noop");
        return Ok(());
    }

    let attack = selected_attack(args.attack)?;
    if args.check_config {
        println!("attack={}", attack.as_str());
        return Ok(());
    }

    match attack {
        Attack::Noop => run_noop(),
    }
}

fn selected_attack(cli_attack: Option<Attack>) -> anyhow::Result<Attack> {
    if let Some(attack) = cli_attack {
        return Ok(attack);
    }
    if let Ok(raw) = std::env::var(ATTACK_ENV) {
        return Attack::parse(&raw);
    }
    if let Some(raw) = proc_cmdline_attack()? {
        return Attack::parse(&raw);
    }
    anyhow::bail!(
        "missing malicious guestd attack; pass --attack, set {ATTACK_ENV}, or add {CMDLINE_KEY}=<name>"
    )
}

fn proc_cmdline_attack() -> anyhow::Result<Option<String>> {
    let cmdline = match std::fs::read_to_string("/proc/cmdline") {
        Ok(cmdline) => cmdline,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("read /proc/cmdline"),
    };
    Ok(cmdline.split_whitespace().find_map(|part| {
        part.strip_prefix(CMDLINE_KEY)
            .and_then(|value| value.strip_prefix('='))
            .map(str::to_owned)
    }))
}

fn run_noop() -> anyhow::Result<()> {
    let listener = VsockListener::bind_with_cid_port(VMADDR_CID_ANY, m80_proto::GUEST_PORT_DEFAULT)
        .with_context(|| {
            format!(
                "failed to bind malicious guestd vsock listener on port {}",
                m80_proto::GUEST_PORT_DEFAULT
            )
        })?;

    let mut ready = VsockStream::connect_with_cid_port(
        VMADDR_CID_HOST,
        m80_proto::READY_PORT_DEFAULT,
    )
    .with_context(|| {
        format!(
            "failed to connect malicious ready signal to host CID {VMADDR_CID_HOST} port {}",
            m80_proto::READY_PORT_DEFAULT
        )
    })?;
    ready
        .write_all(&[m80_proto::PROTOCOL_VERSION as u8])
        .context("failed to write malicious ready version byte")?;
    ready.flush().context("failed to flush malicious ready")?;
    drop(ready);

    loop {
        let (stream, _addr) = listener.accept().context("malicious vsock accept failed")?;
        drop(stream);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> anyhow::Result<Args> {
        parse_args_from(argv.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_attack_flag() {
        let args = parse(&["m80-guestd-malicious", "--attack", "noop"]).unwrap();
        assert_eq!(args.attack, Some(Attack::Noop));
    }

    #[test]
    fn rejects_unknown_attack() {
        let err = parse(&["m80-guestd-malicious", "--attack", "bogus"]).unwrap_err();
        assert!(
            err.to_string().contains("unknown malicious guestd attack"),
            "{err:#}"
        );
    }

    #[test]
    fn parses_proc_cmdline_key_shape() {
        let cmdline = "console=ttyS0 m80.malicious_attack=noop m80.workspace=0";
        let raw = cmdline.split_whitespace().find_map(|part| {
            part.strip_prefix(CMDLINE_KEY)
                .and_then(|value| value.strip_prefix('='))
                .map(str::to_owned)
        });
        assert_eq!(raw.as_deref(), Some("noop"));
    }
}
