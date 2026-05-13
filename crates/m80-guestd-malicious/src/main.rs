//! Test-only adversarial guest daemon for real-KVM guest-to-host wire tests.

use std::io::{Read, Write};

use anyhow::Context as _;
use m80_proto::{FileReadResponse, Payload, RawEnvelope};
use vsock::{VsockListener, VsockStream, VMADDR_CID_ANY, VMADDR_CID_HOST};

const ATTACK_ENV: &str = "M80_MALICIOUS_ATTACK";
const CMDLINE_KEY: &str = "m80.malicious_attack";
const UNSOLICITED_FLOOD_FRAMES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attack {
    Noop,
    OversizedLength,
    TruncatedFrame,
    UnknownVariant,
    ResponseTypeMismatch,
    BogusRequestId,
    UnsolicitedResponse,
    UnsolicitedFlood,
    Slowloris,
}

impl Attack {
    const ALL: [Attack; 9] = [
        Attack::Noop,
        Attack::OversizedLength,
        Attack::TruncatedFrame,
        Attack::UnknownVariant,
        Attack::ResponseTypeMismatch,
        Attack::BogusRequestId,
        Attack::UnsolicitedResponse,
        Attack::UnsolicitedFlood,
        Attack::Slowloris,
    ];

    fn parse(raw: &str) -> anyhow::Result<Self> {
        match raw {
            "noop" => Ok(Attack::Noop),
            "oversized_length" => Ok(Attack::OversizedLength),
            "truncated_frame" => Ok(Attack::TruncatedFrame),
            "unknown_variant" => Ok(Attack::UnknownVariant),
            "response_type_mismatch" => Ok(Attack::ResponseTypeMismatch),
            "bogus_request_id" => Ok(Attack::BogusRequestId),
            "unsolicited_response" => Ok(Attack::UnsolicitedResponse),
            "unsolicited_flood" => Ok(Attack::UnsolicitedFlood),
            "slowloris" => Ok(Attack::Slowloris),
            other => anyhow::bail!("unknown malicious guestd attack: {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Attack::Noop => "noop",
            Attack::OversizedLength => "oversized_length",
            Attack::TruncatedFrame => "truncated_frame",
            Attack::UnknownVariant => "unknown_variant",
            Attack::ResponseTypeMismatch => "response_type_mismatch",
            Attack::BogusRequestId => "bogus_request_id",
            Attack::UnsolicitedResponse => "unsolicited_response",
            Attack::UnsolicitedFlood => "unsolicited_flood",
            Attack::Slowloris => "slowloris",
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
        for attack in Attack::ALL {
            println!("{}", attack.as_str());
        }
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
        | Attack::UnknownVariant
        | Attack::ResponseTypeMismatch
        | Attack::BogusRequestId
        | Attack::UnsolicitedResponse
        | Attack::UnsolicitedFlood
        | Attack::Slowloris => run_peer(attack),
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
            Attack::ResponseTypeMismatch => {
                read_request_then_write_response_type_mismatch(&mut stream)?;
                drop(stream);
            }
            Attack::BogusRequestId => {
                read_request_then_write_bogus_request_id(&mut stream)?;
                drop(stream);
            }
            Attack::UnsolicitedResponse => {
                write_unsolicited_response(&mut stream)?;
                drop(stream);
            }
            Attack::UnsolicitedFlood => {
                write_unsolicited_flood(&mut stream)?;
                drop(stream);
            }
            Attack::Slowloris => {
                write_slowloris_prefix(&mut stream)?;
                loop {
                    std::thread::park();
                }
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
    let envelope = RawEnvelope {
        version: m80_proto::PROTOCOL_VERSION,
        kind: "unknown_variant".to_owned(),
        request_id: None,
        max_duration_ms: None,
        payload: m80_proto::PingRequest {}.into_wire(),
    };
    let mut body = m80_proto::encode_raw_envelope(envelope)
        .context("encode known envelope before unknown-variant mutation")?;
    mutate_ping_payload_tag_to_unknown(&mut body)?;

    let len = u32::try_from(body.len()).context("unknown-variant frame length fits u32")?;
    stream
        .write_all(&len.to_be_bytes())
        .context("write unknown-variant frame length")?;
    stream
        .write_all(&body)
        .context("write unknown-variant frame body")?;
    stream.flush().context("flush unknown-variant frame")
}

fn mutate_ping_payload_tag_to_unknown(body: &mut [u8]) -> anyhow::Result<()> {
    const PING_REQUEST_TAG: [u8; 2] = [0x9a, 0x03];
    const UNKNOWN_PAYLOAD_TAG: [u8; 2] = [0xfa, 0x0f];
    let offset = body
        .windows(PING_REQUEST_TAG.len())
        .position(|window| window == PING_REQUEST_TAG)
        .context("encoded ping_request payload tag missing")?;
    body[offset..offset + UNKNOWN_PAYLOAD_TAG.len()].copy_from_slice(&UNKNOWN_PAYLOAD_TAG);
    Ok(())
}

fn read_request_then_write_response_type_mismatch<S>(stream: &mut S) -> anyhow::Result<()>
where
    S: Read + Write,
{
    let request =
        m80_proto::read_raw_frame(stream).context("read request before mismatch frame")?;
    write_response_type_mismatch(stream, request)
}

fn write_response_type_mismatch(
    stream: &mut impl Write,
    request: RawEnvelope,
) -> anyhow::Result<()> {
    let envelope = RawEnvelope {
        version: m80_proto::PROTOCOL_VERSION,
        kind: m80_proto::PAYLOAD_KIND_EXEC_EXIT.to_owned(),
        request_id: request.request_id,
        max_duration_ms: None,
        payload: FileReadResponse {
            bytes: Vec::new(),
            truncated: false,
            error: None,
        }
        .into_wire(),
    };
    m80_proto::write_raw_frame(stream, envelope).context("write response-type-mismatch frame")?;
    stream.flush().context("flush response-type-mismatch frame")
}

fn read_request_then_write_bogus_request_id<S>(stream: &mut S) -> anyhow::Result<()>
where
    S: Read + Write,
{
    let _request =
        m80_proto::read_raw_frame(stream).context("read request before bogus request-id frame")?;
    write_bogus_request_id(stream)
}

fn write_bogus_request_id(stream: &mut impl Write) -> anyhow::Result<()> {
    write_exec_exit_for_request_id(stream, "malicious-stale-request-id", "bogus-request-id")
}

fn write_unsolicited_response(stream: &mut impl Write) -> anyhow::Result<()> {
    write_exec_exit_for_request_id(stream, "unsolicited-response", "unsolicited-response")
}

fn write_unsolicited_flood(stream: &mut impl Write) -> anyhow::Result<()> {
    for i in 0..UNSOLICITED_FLOOD_FRAMES {
        write_exec_exit_for_request_id(
            stream,
            &format!("unsolicited-flood-{i}"),
            "unsolicited-flood",
        )?;
    }
    Ok(())
}

fn write_slowloris_prefix(stream: &mut impl Write) -> anyhow::Result<()> {
    const DECLARED_LEN: u32 = 16;
    stream
        .write_all(&DECLARED_LEN.to_be_bytes())
        .context("write slowloris frame length")?;
    stream
        .write_all(b"slow")
        .context("write slowloris partial body")?;
    stream.flush().context("flush slowloris partial body")
}

fn write_exec_exit_for_request_id(
    stream: &mut impl Write,
    request_id: &str,
    context: &str,
) -> anyhow::Result<()> {
    let envelope = RawEnvelope {
        version: m80_proto::PROTOCOL_VERSION,
        kind: m80_proto::PAYLOAD_KIND_EXEC_EXIT.to_owned(),
        request_id: Some(request_id.to_owned()),
        max_duration_ms: None,
        payload: m80_proto::ExecExit {
            status: m80_proto::ExecStatus::Completed,
            exit_code: Some(0),
            total_stdout_bytes: 0,
            total_stderr_bytes: 0,
            truncated: false,
            timing: m80_proto::ExecTiming {
                spawned_at_unix_ms: 1,
                exited_at_unix_ms: 2,
                spawn_ms: 0,
                run_ms: 1,
            },
        }
        .into_wire(),
    };
    m80_proto::write_raw_frame(stream, envelope)
        .with_context(|| format!("write {context} frame"))?;
    stream
        .flush()
        .with_context(|| format!("flush {context} frame"))
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

    #[test]
    fn response_type_mismatch_echoes_request_id_with_wrong_payload_shape() {
        let typed = m80_proto::Envelope::with_request_id(
            m80_proto::ExecRequest {
                program: "/bin/true".into(),
                args: Vec::new(),
                cwd: None,
                env: None,
                stdin: None,
                timeout_ms: None,
                streaming: true,
            },
            "req-response-mismatch".into(),
        );
        let request = RawEnvelope::from_typed(&typed);
        let mut frame = Vec::new();
        write_response_type_mismatch(&mut frame, request).unwrap();

        let raw = m80_proto::read_raw_frame(&mut std::io::Cursor::new(frame)).unwrap();
        assert_eq!(raw.kind, m80_proto::PAYLOAD_KIND_EXEC_EXIT);
        assert_eq!(raw.request_id.as_deref(), Some("req-response-mismatch"));
        let err = raw.clone().decode::<m80_proto::ExecExit>().unwrap_err();
        assert!(
            err.to_string()
                .contains("unexpected protobuf payload for exec_exit: file_read_response"),
            "{err}"
        );
        assert!(matches!(
            raw.payload,
            m80_proto::wire::WirePayload::FileReadResponse(_)
        ));
    }

    #[test]
    fn bogus_request_id_writes_valid_exit_for_fabricated_request() {
        let mut frame = Vec::new();
        write_bogus_request_id(&mut frame).unwrap();

        let raw = m80_proto::read_raw_frame(&mut std::io::Cursor::new(frame)).unwrap();
        assert_eq!(raw.kind, m80_proto::PAYLOAD_KIND_EXEC_EXIT);
        assert_eq!(
            raw.request_id.as_deref(),
            Some("malicious-stale-request-id")
        );
        raw.decode::<m80_proto::ExecExit>().unwrap();
    }

    #[test]
    fn unsolicited_response_writes_exit_without_reading_a_request() {
        let mut frame = Vec::new();
        write_unsolicited_response(&mut frame).unwrap();

        let raw = m80_proto::read_raw_frame(&mut std::io::Cursor::new(frame)).unwrap();
        assert_eq!(raw.kind, m80_proto::PAYLOAD_KIND_EXEC_EXIT);
        assert_eq!(raw.request_id.as_deref(), Some("unsolicited-response"));
        raw.decode::<m80_proto::ExecExit>().unwrap();
    }

    #[test]
    fn unsolicited_flood_writes_bounded_fabricated_responses() {
        let mut frames = Vec::new();
        write_unsolicited_flood(&mut frames).unwrap();

        let mut cursor = std::io::Cursor::new(frames);
        let first = m80_proto::read_raw_frame(&mut cursor).unwrap();
        assert_eq!(first.request_id.as_deref(), Some("unsolicited-flood-0"));
        first.decode::<m80_proto::ExecExit>().unwrap();

        let second = m80_proto::read_raw_frame(&mut cursor).unwrap();
        assert_eq!(second.request_id.as_deref(), Some("unsolicited-flood-1"));
        second.decode::<m80_proto::ExecExit>().unwrap();
    }

    #[test]
    fn slowloris_prefix_writes_short_incomplete_frame() {
        let mut bytes = Vec::new();
        write_slowloris_prefix(&mut bytes).unwrap();
        let declared = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(declared, 16);
        assert_eq!(&bytes[4..], b"slow");
        assert!(bytes[4..].len() < declared);
    }
}
