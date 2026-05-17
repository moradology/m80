//! Host-side post-restore hook RPC.

use std::fs::File;
use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, Instant};

use m80_proto::{
    Envelope, HookError, HookKindWire, HookResultWire, HookStatus, PostRestoreHookRequest,
    PostRestoreHookResponse, RawEnvelope,
};
use m80_vsock::Channel;

use crate::error::{ConfigError, FcError, WireProtocolError};
use crate::warm_pool::{HookSpec, HookSpecSet};

const RESTORE_NONCE_BYTES: usize = 32;
const POST_RESTORE_HOOK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Ask guestd to mix host restore entropy and run the ordered hook set before
/// the restored VM is handed to callers.
pub(crate) fn phase_restore_post_restore_hooks(
    vsock_uds: &Path,
    vm_id: &str,
    base_request_id: Option<&str>,
    firecracker_pid: u32,
    hooks: &HookSpecSet,
) -> Result<(), FcError> {
    if hooks.hooks().is_empty() {
        return Ok(());
    }

    let restore_nonce = restore_nonce()?;
    send_post_restore_hooks(
        vsock_uds,
        vm_id,
        base_request_id,
        firecracker_pid,
        hooks,
        restore_nonce,
    )
}

fn send_post_restore_hooks(
    vsock_uds: &Path,
    vm_id: &str,
    base_request_id: Option<&str>,
    firecracker_pid: u32,
    hooks: &HookSpecSet,
    restore_nonce: [u8; RESTORE_NONCE_BYTES],
) -> Result<(), FcError> {
    let expected_hooks = wire_hooks(hooks)?;
    let request_id = super::exec::request_id_for(vm_id, base_request_id, "post_restore");
    let envelope = Envelope::with_request_id(
        PostRestoreHookRequest {
            restore_nonce,
            hooks: expected_hooks.clone(),
        },
        request_id.clone(),
    );
    let mut channel = super::exec::send_envelope_with_open_retry(
        vsock_uds,
        vm_id,
        firecracker_pid,
        "post-restore hooks",
        &envelope,
    )?;
    let response = recv_post_restore_response(
        &mut channel,
        firecracker_pid,
        POST_RESTORE_HOOK_RESPONSE_TIMEOUT,
    )?;
    validate_post_restore_response(&response, &request_id, &expected_hooks)
}

fn recv_post_restore_response(
    channel: &mut Channel,
    firecracker_pid: u32,
    timeout: Duration,
) -> Result<Envelope<PostRestoreHookResponse>, FcError> {
    let deadline = Instant::now() + timeout;
    let raw = match channel.recv_raw_with_deadline(deadline) {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            return Err(FcError::Protocol(WireProtocolError::ReadTimeout {
                context: "post-restore hooks",
            }));
        }
        Err(err) => {
            return Err(super::protocol::recv_error(
                err,
                "post-restore hooks",
                firecracker_pid,
            ))
        }
    };
    decode_post_restore_response(raw)
}

fn decode_post_restore_response(
    raw: RawEnvelope,
) -> Result<Envelope<PostRestoreHookResponse>, FcError> {
    raw.decode::<PostRestoreHookResponse>()
        .map_err(super::protocol::proto_error)
}

fn restore_nonce() -> Result<[u8; RESTORE_NONCE_BYTES], FcError> {
    let mut file = File::open("/dev/urandom").map_err(|source| FcError::HostIo {
        operation: "open /dev/urandom for restore nonce",
        source,
    })?;
    let mut nonce = [0u8; RESTORE_NONCE_BYTES];
    file.read_exact(&mut nonce)
        .map_err(|source| FcError::HostIo {
            operation: "read restore nonce from /dev/urandom",
            source,
        })?;
    Ok(nonce)
}

fn wire_hooks(hooks: &HookSpecSet) -> Result<Vec<HookKindWire>, FcError> {
    hooks
        .hooks()
        .iter()
        .map(|hook| match hook {
            HookSpec::ReseedSystemdRandomSeed => Ok(HookKindWire::ReseedSystemdRandomSeed),
            HookSpec::RegenMachineId => Ok(HookKindWire::RegenMachineId),
            HookSpec::SetHostname(hostname) => HookKindWire::set_hostname(hostname.as_str())
                .map_err(|err| {
                    FcError::Config(ConfigError::InvalidValue {
                        field: "hook.hostname",
                        reason: err.to_string(),
                    })
                }),
        })
        .collect()
}

fn validate_post_restore_response(
    response: &Envelope<PostRestoreHookResponse>,
    request_id: &str,
    expected_hooks: &[HookKindWire],
) -> Result<(), FcError> {
    if response.request_id.as_deref() != Some(request_id) {
        return Err(super::protocol::request_id_mismatch(
            "post-restore hooks",
            request_id,
            response.request_id.clone(),
        ));
    }
    if response.payload.results.len() != expected_hooks.len() {
        return Err(super::protocol::unexpected_frame(
            "post-restore hooks",
            "one result per requested hook",
            format!("{} results", response.payload.results.len()),
        ));
    }

    for (index, (result, expected)) in response
        .payload
        .results
        .iter()
        .zip(expected_hooks.iter())
        .enumerate()
    {
        validate_hook_result(index, result, expected)?;
    }
    Ok(())
}

fn validate_hook_result(
    index: usize,
    result: &HookResultWire,
    expected: &HookKindWire,
) -> Result<(), FcError> {
    if &result.kind != expected {
        return Err(super::protocol::unexpected_frame(
            "post-restore hooks",
            "matching hook result kind",
            format!("result {index} was {:?}", result.kind),
        ));
    }
    match (result.status, result.error) {
        (HookStatus::Succeeded, None) => Ok(()),
        (HookStatus::Succeeded, Some(error)) => Err(super::protocol::unexpected_frame(
            "post-restore hooks",
            "succeeded hook result without error",
            format!("result {index} carried {error:?}"),
        )),
        (HookStatus::Failed, Some(error)) => Err(FcError::PostRestoreHook(error)),
        (HookStatus::Failed, None) => Err(FcError::PostRestoreHook(HookError::ReseedFailed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostnameSpec, WireProtocolError};
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use std::time::Duration;

    fn hooks() -> HookSpecSet {
        HookSpecSet::new(vec![
            HookSpec::ReseedSystemdRandomSeed,
            HookSpec::RegenMachineId,
            HookSpec::SetHostname(HostnameSpec::new("lease-1").unwrap()),
        ])
    }

    fn response(
        request_id: Option<&str>,
        results: Vec<HookResultWire>,
    ) -> Envelope<PostRestoreHookResponse> {
        let payload = PostRestoreHookResponse { results };
        match request_id {
            Some(id) => Envelope::with_request_id(payload, id.to_owned()),
            None => Envelope::new(payload),
        }
    }

    fn success(kind: HookKindWire) -> HookResultWire {
        HookResultWire {
            kind,
            status: HookStatus::Succeeded,
            error: None,
        }
    }

    #[test]
    fn wire_hooks_preserve_order_and_hostname() {
        let converted = wire_hooks(&hooks()).unwrap();

        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0], HookKindWire::ReseedSystemdRandomSeed);
        assert_eq!(converted[1], HookKindWire::RegenMachineId);
        match &converted[2] {
            HookKindWire::SetHostname { hostname } => {
                assert_eq!(hostname.as_str(), "lease-1");
            }
            other => panic!("expected hostname hook, got {other:?}"),
        }
    }

    #[test]
    fn response_requires_matching_request_id() {
        let expected = wire_hooks(&hooks()).unwrap();
        let response = response(
            Some("wrong"),
            expected.iter().cloned().map(success).collect(),
        );

        let err = validate_post_restore_response(&response, "expected", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::RequestIdMismatch {
                context: "post-restore hooks",
                ..
            })
        ));
    }

    #[test]
    fn response_requires_one_result_per_hook() {
        let expected = wire_hooks(&hooks()).unwrap();
        let response = response(Some("req"), Vec::new());

        let err = validate_post_restore_response(&response, "req", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::UnexpectedFrame {
                context: "post-restore hooks",
                ..
            })
        ));
    }

    #[test]
    fn response_rejects_mismatched_hook_kind() {
        let expected = wire_hooks(&hooks()).unwrap();
        let mut results: Vec<_> = expected.iter().cloned().map(success).collect();
        results[1].kind = HookKindWire::ReseedSystemdRandomSeed;
        let response = response(Some("req"), results);

        let err = validate_post_restore_response(&response, "req", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::UnexpectedFrame {
                context: "post-restore hooks",
                ..
            })
        ));
    }

    #[test]
    fn response_returns_typed_guest_hook_error() {
        let expected = wire_hooks(&hooks()).unwrap();
        let mut results: Vec<_> = expected.iter().cloned().map(success).collect();
        results[1].status = HookStatus::Failed;
        results[1].error = Some(HookError::MachineIdWriteFailed);
        let response = response(Some("req"), results);

        let err = validate_post_restore_response(&response, "req", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::PostRestoreHook(HookError::MachineIdWriteFailed)
        ));
    }

    #[test]
    fn response_rejects_success_with_error_detail() {
        let expected = wire_hooks(&hooks()).unwrap();
        let mut results: Vec<_> = expected.iter().cloned().map(success).collect();
        results[0].error = Some(HookError::ReseedFailed);
        let response = response(Some("req"), results);

        let err = validate_post_restore_response(&response, "req", &expected).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::UnexpectedFrame {
                context: "post-restore hooks",
                ..
            })
        ));
    }

    #[test]
    fn recv_post_restore_response_times_out_when_guest_never_replies() {
        let dir = tempfile::tempdir().expect("uds tempdir");
        let sock = dir.path().join("fc.sock");
        let listener = UnixListener::bind(&sock).expect("bind fake firecracker uds");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept channel");
            stream.write_all(b"OK 3\n").expect("write handshake");
            stream.flush().expect("flush handshake");
            thread::sleep(Duration::from_millis(100));
        });

        let mut channel = Channel::open_uds_only(&sock, 1024).expect("open channel");
        let err = recv_post_restore_response(&mut channel, u32::MAX, Duration::from_millis(10))
            .expect_err("missing hook response must time out");

        assert!(matches!(
            err,
            FcError::Protocol(WireProtocolError::ReadTimeout {
                context: "post-restore hooks"
            })
        ));
        server.join().expect("server thread");
    }
}
