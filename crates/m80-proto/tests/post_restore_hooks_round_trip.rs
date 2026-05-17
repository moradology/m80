use std::io::Cursor;

use m80_proto::wire::generated::wire_hook_kind::Kind as WireHookKindKind;
use m80_proto::wire::generated::{WireHookKind, WirePostRestoreHookRequest};
use m80_proto::wire::WirePayload;
use m80_proto::{
    read_frame, write_frame, Envelope, HookError, HookKindWire, HookResultWire, HookStatus,
    PostRestoreHookRequest, PostRestoreHookResponse, ProtoError, RawEnvelope,
    PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST, PROTOCOL_VERSION,
};

fn round_trip<T>(payload: T) -> Envelope<T>
where
    T: m80_proto::Payload + Clone,
{
    let env = Envelope::with_request_id(payload, "req-hooks".to_owned());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    read_frame(&mut Cursor::new(bytes)).unwrap()
}

fn nonce() -> [u8; 32] {
    let mut nonce = [0_u8; 32];
    for (i, byte) in nonce.iter_mut().enumerate() {
        *byte = i as u8;
    }
    nonce
}

#[test]
fn post_restore_hook_request_round_trips_all_hook_variants() {
    let decoded = round_trip(PostRestoreHookRequest {
        restore_nonce: nonce(),
        hooks: vec![
            HookKindWire::ReseedSystemdRandomSeed,
            HookKindWire::RegenMachineId,
            HookKindWire::set_hostname("lease-1.example").expect("valid hostname"),
        ],
    });

    assert_eq!(decoded.request_id.as_deref(), Some("req-hooks"));
    assert_eq!(decoded.payload.restore_nonce, nonce());
    assert_eq!(decoded.payload.hooks.len(), 3);
    assert_eq!(
        decoded.payload.hooks[0],
        HookKindWire::ReseedSystemdRandomSeed
    );
    assert_eq!(decoded.payload.hooks[1], HookKindWire::RegenMachineId);
    assert_eq!(
        decoded.payload.hooks[2],
        HookKindWire::set_hostname("lease-1.example").expect("valid hostname")
    );
}

#[test]
fn post_restore_hook_response_round_trips_statuses_and_errors() {
    let hostname = HookKindWire::set_hostname("lease-2").expect("valid hostname");
    let decoded = round_trip(PostRestoreHookResponse {
        results: vec![
            HookResultWire {
                kind: HookKindWire::ReseedSystemdRandomSeed,
                status: HookStatus::Succeeded,
                error: None,
            },
            HookResultWire {
                kind: HookKindWire::ReseedSystemdRandomSeed,
                status: HookStatus::Failed,
                error: Some(HookError::ReseedFailed),
            },
            HookResultWire {
                kind: HookKindWire::RegenMachineId,
                status: HookStatus::Failed,
                error: Some(HookError::MachineIdWriteFailed),
            },
            HookResultWire {
                kind: hostname.clone(),
                status: HookStatus::Failed,
                error: Some(HookError::HostnameSyscallFailed { errno: 22 }),
            },
            HookResultWire {
                kind: hostname.clone(),
                status: HookStatus::Failed,
                error: Some(HookError::HostnameWriteFailed),
            },
            HookResultWire {
                kind: HookKindWire::ReseedSystemdRandomSeed,
                status: HookStatus::Failed,
                error: Some(HookError::RandomSeedWriteFailed),
            },
            HookResultWire {
                kind: hostname,
                status: HookStatus::Failed,
                error: Some(HookError::InvalidHostname),
            },
        ],
    });

    assert_eq!(decoded.payload.results.len(), 7);
    assert_eq!(decoded.payload.results[0].status, HookStatus::Succeeded);
    assert_eq!(decoded.payload.results[0].error, None);
    assert_eq!(
        decoded.payload.results[3].error,
        Some(HookError::HostnameSyscallFailed { errno: 22 })
    );
    assert_eq!(
        decoded.payload.results[6].error,
        Some(HookError::InvalidHostname)
    );
}

#[test]
fn post_restore_hook_request_rejects_wrong_length_restore_nonce() {
    let err = raw_request(vec![7; 31], Vec::new())
        .decode::<PostRestoreHookRequest>()
        .expect_err("wrong nonce length must fail");

    assert_malformed_contains(err, "restore_nonce must be exactly 32 bytes");
}

#[test]
fn post_restore_hook_request_rejects_invalid_hostname_at_decode() {
    let err = raw_request(
        nonce().to_vec(),
        vec![WireHookKind {
            kind: Some(WireHookKindKind::SetHostname("-bad".to_owned())),
        }],
    )
    .decode::<PostRestoreHookRequest>()
    .expect_err("invalid hostname must fail");

    assert_malformed_contains(err, "invalid post-restore hostname");
}

fn raw_request(restore_nonce: Vec<u8>, hooks: Vec<WireHookKind>) -> RawEnvelope {
    RawEnvelope {
        version: PROTOCOL_VERSION,
        kind: PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST.to_owned(),
        request_id: None,
        max_duration_ms: None,
        payload: WirePayload::PostRestoreHookRequest(WirePostRestoreHookRequest {
            restore_nonce,
            hooks,
        }),
    }
}

fn assert_malformed_contains(err: ProtoError, needle: &str) {
    match err {
        ProtoError::MalformedPayload(message) => assert!(
            message.contains(needle),
            "expected {message:?} to contain {needle:?}"
        ),
        other => panic!("expected malformed payload, got {other:?}"),
    }
}
