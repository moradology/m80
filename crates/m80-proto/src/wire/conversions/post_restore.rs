use crate::types::{
    HookError, HookKindWire, HookResultWire, HookStatus, PostRestoreHookRequest,
    PostRestoreHookResponse, PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST,
    PAYLOAD_KIND_POST_RESTORE_HOOK_RESPONSE,
};

use super::{
    missing, payload_name, Payload, ProtoError, WireHookError, WireHookErrorKind, WireHookKind,
    WireHookKindKind, WireHookResult, WirePayload, WirePostRestoreHookRequest,
    WirePostRestoreHookResponse,
};

payload_impl!(
    PostRestoreHookRequest,
    PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST,
    PostRestoreHookRequest,
    WirePostRestoreHookRequest,
    post_restore_hook_request_to_wire,
    post_restore_hook_request_from_wire
);

payload_impl!(
    PostRestoreHookResponse,
    PAYLOAD_KIND_POST_RESTORE_HOOK_RESPONSE,
    PostRestoreHookResponse,
    WirePostRestoreHookResponse,
    post_restore_hook_response_to_wire,
    post_restore_hook_response_from_wire
);

pub(super) fn post_restore_hook_request_to_wire(
    value: PostRestoreHookRequest,
) -> WirePostRestoreHookRequest {
    WirePostRestoreHookRequest {
        restore_nonce: value.restore_nonce.to_vec(),
        hooks: value.hooks.into_iter().map(hook_kind_to_wire).collect(),
    }
}

pub(super) fn post_restore_hook_request_from_wire(
    value: WirePostRestoreHookRequest,
) -> Result<PostRestoreHookRequest, ProtoError> {
    let restore_nonce = <[u8; 32]>::try_from(value.restore_nonce.as_slice()).map_err(|_| {
        ProtoError::MalformedPayload("restore_nonce must be exactly 32 bytes".into())
    })?;
    Ok(PostRestoreHookRequest {
        restore_nonce,
        hooks: value
            .hooks
            .into_iter()
            .map(hook_kind_from_wire)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(super) fn post_restore_hook_response_to_wire(
    value: PostRestoreHookResponse,
) -> WirePostRestoreHookResponse {
    WirePostRestoreHookResponse {
        results: value.results.into_iter().map(hook_result_to_wire).collect(),
    }
}

pub(super) fn post_restore_hook_response_from_wire(
    value: WirePostRestoreHookResponse,
) -> Result<PostRestoreHookResponse, ProtoError> {
    Ok(PostRestoreHookResponse {
        results: value
            .results
            .into_iter()
            .map(hook_result_from_wire)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn hook_kind_to_wire(value: HookKindWire) -> WireHookKind {
    WireHookKind {
        kind: Some(match value {
            HookKindWire::ReseedSystemdRandomSeed => {
                WireHookKindKind::ReseedSystemdRandomSeed(true)
            }
            HookKindWire::RegenMachineId => WireHookKindKind::RegenMachineId(true),
            HookKindWire::SetHostname { hostname } => {
                WireHookKindKind::SetHostname(hostname.into_string())
            }
        }),
    }
}

fn hook_kind_from_wire(value: WireHookKind) -> Result<HookKindWire, ProtoError> {
    match value.kind.ok_or_else(|| missing("hook kind"))? {
        WireHookKindKind::ReseedSystemdRandomSeed(_) => Ok(HookKindWire::ReseedSystemdRandomSeed),
        WireHookKindKind::RegenMachineId(_) => Ok(HookKindWire::RegenMachineId),
        WireHookKindKind::SetHostname(hostname) => HookKindWire::set_hostname(hostname),
    }
}

fn hook_result_to_wire(value: HookResultWire) -> WireHookResult {
    WireHookResult {
        kind: Some(hook_kind_to_wire(value.kind)),
        status: hook_status_to_i32(value.status),
        error: value.error.map(hook_error_to_wire),
    }
}

fn hook_result_from_wire(value: WireHookResult) -> Result<HookResultWire, ProtoError> {
    let status = hook_status_from_i32(value.status)?;
    let error = value.error.map(hook_error_from_wire).transpose()?;
    match (status, error) {
        (HookStatus::Succeeded, Some(_)) => Err(ProtoError::MalformedPayload(
            "successful hook result must not carry error".into(),
        )),
        (HookStatus::Failed, None) => Err(ProtoError::MalformedPayload(
            "failed hook result must carry error".into(),
        )),
        (status, error) => Ok(HookResultWire {
            kind: hook_kind_from_wire(value.kind.ok_or_else(|| missing("hook result kind"))?)?,
            status,
            error,
        }),
    }
}

fn hook_status_to_i32(value: HookStatus) -> i32 {
    match value {
        HookStatus::Succeeded => 0,
        HookStatus::Failed => 1,
    }
}

fn hook_status_from_i32(value: i32) -> Result<HookStatus, ProtoError> {
    match value {
        0 => Ok(HookStatus::Succeeded),
        1 => Ok(HookStatus::Failed),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown hook status: {value}"
        ))),
    }
}

fn hook_error_to_wire(value: HookError) -> WireHookError {
    WireHookError {
        error: Some(match value {
            HookError::ReseedFailed => WireHookErrorKind::ReseedFailed(true),
            HookError::MachineIdWriteFailed => WireHookErrorKind::MachineIdWriteFailed(true),
            HookError::HostnameSyscallFailed { errno } => {
                WireHookErrorKind::HostnameSyscallFailedErrno(errno)
            }
            HookError::HostnameWriteFailed => WireHookErrorKind::HostnameWriteFailed(true),
            HookError::RandomSeedWriteFailed => WireHookErrorKind::RandomSeedWriteFailed(true),
            HookError::InvalidHostname => WireHookErrorKind::InvalidHostname(true),
        }),
    }
}

fn hook_error_from_wire(value: WireHookError) -> Result<HookError, ProtoError> {
    match value.error.ok_or_else(|| missing("hook error"))? {
        WireHookErrorKind::ReseedFailed(_) => Ok(HookError::ReseedFailed),
        WireHookErrorKind::MachineIdWriteFailed(_) => Ok(HookError::MachineIdWriteFailed),
        WireHookErrorKind::HostnameSyscallFailedErrno(errno) => {
            Ok(HookError::HostnameSyscallFailed { errno })
        }
        WireHookErrorKind::HostnameWriteFailed(_) => Ok(HookError::HostnameWriteFailed),
        WireHookErrorKind::RandomSeedWriteFailed(_) => Ok(HookError::RandomSeedWriteFailed),
        WireHookErrorKind::InvalidHostname(_) => Ok(HookError::InvalidHostname),
    }
}
