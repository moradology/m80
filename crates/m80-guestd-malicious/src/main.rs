//! Test-only adversarial guest daemon for real-KVM guest-to-host wire tests.

use std::io::Write;

use anyhow::Context as _;
use vsock::{VsockListener, VsockStream, VMADDR_CID_ANY, VMADDR_CID_HOST};

const ATTACK_ENV: &str = "M80_MALICIOUS_ATTACK";
const CMDLINE_KEY: &str = "m80.malicious_attack";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attack {
    Noop,
    OversizedLength,
    TruncatedFrame,
    UnknownVariant,
}

impl Attack {
    fn parse(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "noop" => Ok(Attack::Noop),
            "oversized_length" => Ok(Attack::OversizedLength),
            "truncated_frame" => Ok(Attack::TruncatedFrame),
            "unknown_variant" => Ok(Attack::UnknownVariant),
            other => anyhow::bail!("unknown malicious guestd attack: {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Attack::Noop => "noop",
            Attack::OversizedLength => "oversized_length",
            Attack::TruncatedFrame => "truncated_frame",
            Attack::UnknownVariant => "unknown_variant",
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
        println!("oversized_length");
        println!("truncated_frame");
        println!("unknown_variant");
        return Ok(());
    }

    let attack = selected_attack(args.attack)?;
    if args.check_config {
        println!("attack={}", attack.as_str());
        return Ok(());
    }

    match attack {
        Attack::Noop
        | Attack::OversizedLength
        | Attack::TruncatedFrame
        | Attack::UnknownVariant => run_peer(attack),
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

fn run_peer(attack: Attack) -> anyhow::Result<()> {
    let listener = VsockListener::bind_with_cid_port(VMADDR_CID_ANY, m80_proto::GUEST_PORT_DEFAULT)
        .with_context(|| {
            format!(
                "failed to bind malicious guestd vsock listener on port {}",
                m80_proto::GUEST_PORT_DEFAULT
            )
        })?;

    let mut ready =
        VsockStream::connect_with_cid_port(VMADDR_CID_HOST, m80_proto::READY_PORT_DEFAULT)
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
        let (mut stream, _addr) = listener.accept().context("malicious vsock accept failed")?;
        match attack {
            Attack::Noop => drop(stream),
            Attack::OversizedLength => {
                write_oversized_length(&mut stream)?;
                drop(stream);
            }
            Attack::TruncatedFrame => {
                write_truncated_frame(&mut stream)?;
                drop(stream);
            }
            Attack::UnknownVariant => {
                write_unknown_variant(&mut stream)?;
                drop(stream);
            }
        }
    }
}

fn write_oversized_length(stream: &mut impl Write) -> anyhow::Result<()> {
    let size = u32::try_from(m80_proto::MAX_FRAME_BYTES + 1)
        .context("MAX_FRAME_BYTES + 1 must fit in u32")?;
    stream
        .write_all(&size.to_be_bytes())
        .context("write oversized length prefix")?;
    stream.flush().context("flush oversized length prefix")
}

fn write_truncated_frame(stream: &mut impl Write) -> anyhow::Result<()> {
    const DECLARED_LEN: u32 = 16;
    const WRITTEN_BODY: &[u8] = b"truncated";
    stream
        .write_all(&DECLARED_LEN.to_be_bytes())
        .context("write truncated frame length")?;
    stream
        .write_all(WRITTEN_BODY)
        .context("write truncated frame partial body")?;
    stream.flush().context("flush truncated frame")
}

fn write_unknown_variant(stream: &mut impl Write) -> anyhow::Result<()> {
    let mut body = Vec::new();
    write_varint(&mut body, 1 << 3);
    write_varint(&mut body, u64::from(m80_proto::PROTOCOL_VERSION));
    write_len_field(&mut body, 2, "unknown_variant".as_bytes())?;
    write_varint(&mut body, (255 << 3) | 2);
    write_varint(&mut body, 0);

    let len = u32::try_from(body.len()).context("unknown-variant frame length fits u32")?;
    stream
        .write_all(&len.to_be_bytes())
        .context("write unknown-variant frame length")?;
    stream
        .write_all(&body)
        .context("write unknown-variant frame body")?;
    stream.flush().context("flush unknown-variant frame")
}

fn write_len_field(out: &mut Vec<u8>, field: u64, bytes: &[u8]) -> anyhow::Result<()> {
    write_varint(out, (field << 3) | 2);
    write_varint(
        out,
        u64::try_from(bytes.len()).context("field length fits u64")?,
    );
    out.extend_from_slice(bytes);
    Ok(())
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
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

    #[test]
    fn oversized_length_writes_only_prefix() {
        let mut bytes = Vec::new();
        write_oversized_length(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 4);
        assert_eq!(
            u32::from_be_bytes(bytes.try_into().unwrap()) as usize,
            m80_proto::MAX_FRAME_BYTES + 1
        );
    }

    #[test]
    fn truncated_frame_writes_short_body() {
        let mut bytes = Vec::new();
        write_truncated_frame(&mut bytes).unwrap();
        let declared = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(declared, 16);
        assert!(bytes[4..].len() < declared);
    }

    #[test]
    fn unknown_variant_frame_carries_unknown_payload_field() {
        let mut frame = Vec::new();
        write_unknown_variant(&mut frame).unwrap();
        let declared = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(declared, frame[4..].len());
        assert!(m80_proto::read_raw_frame(&mut std::io::Cursor::new(frame))
            .unwrap_err()
            .to_string()
            .contains("unknown envelope field: 255"));
    }
}
