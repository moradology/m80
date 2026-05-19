use std::os::unix::net::UnixStream;
use std::path::Path;

use m80_firecracker::{FcError, WarmLease, WarmPool};
use m80_proto::{ExecRequest, ExecResponse};

use super::control::{
    self, WarmControlResponse, WarmErrorResponse, WarmRunResult, WarmStreamFrame,
};
use super::status::{self, WarmOwnerIdentity};
use crate::cmds::proto_json::ExecRequestJson;

pub(super) fn handle_run(
    pool: &WarmPool,
    identity: &WarmOwnerIdentity,
    profile: Option<String>,
    egress: &str,
    request_id: String,
    request: ExecRequestJson,
    accepting_leases: bool,
) -> WarmControlResponse {
    handle_run_with_pool(
        pool,
        identity,
        profile,
        egress,
        request_id,
        request,
        accepting_leases,
    )
}

fn handle_run_with_pool<P>(
    pool: &P,
    identity: &WarmOwnerIdentity,
    profile: Option<String>,
    egress: &str,
    request_id: String,
    request: ExecRequestJson,
    accepting_leases: bool,
) -> WarmControlResponse
where
    P: WarmRunPool,
{
    if let Err(e) = validate_run_compatibility(identity, profile, egress, accepting_leases) {
        return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
            &e,
            Some(request_id),
        ));
    }

    let mut lease = match pool.try_lease() {
        Ok(lease) => lease,
        Err(e) => {
            return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
                &e,
                Some(request_id),
            ))
        }
    };
    let run_dir = lease.run_dir().display().to_string();
    // BlankVmReset evidence is not wired in v0.1; leases always discard.
    let reset_decision = "Discard".to_owned();
    let discard_reason = "ResetEvidenceUnavailable".to_owned();
    let response = match lease.exec_with_request_id(request.into(), request_id.clone()) {
        Ok(response) => response,
        Err(e) => {
            let _ = lease.discard();
            return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
                &e,
                Some(request_id),
            ));
        }
    };
    if let Err(e) = lease.discard() {
        return WarmControlResponse::Error(WarmErrorResponse::from_error_with_request_id(
            &e,
            Some(request_id),
        ));
    }
    WarmControlResponse::Run(WarmRunResult {
        request_id,
        response: response.into(),
        reset_decision,
        discard_reason,
        run_dir,
    })
}

trait WarmRunPool {
    type Lease: WarmRunLease;

    fn try_lease(&self) -> Result<Self::Lease, FcError>;
}

trait WarmRunLease {
    fn run_dir(&self) -> &Path;

    fn exec_with_request_id(
        &mut self,
        request: ExecRequest,
        request_id: String,
    ) -> Result<ExecResponse, FcError>;

    fn discard(self) -> Result<(), FcError>;
}

impl WarmRunPool for WarmPool {
    type Lease = WarmLease;

    fn try_lease(&self) -> Result<Self::Lease, FcError> {
        WarmPool::try_lease(self)
    }
}

impl WarmRunLease for WarmLease {
    fn run_dir(&self) -> &Path {
        WarmLease::run_dir(self)
    }

    fn exec_with_request_id(
        &mut self,
        request: ExecRequest,
        request_id: String,
    ) -> Result<ExecResponse, FcError> {
        WarmLease::exec_with_request_id(self, request, request_id)
    }

    fn discard(self) -> Result<(), FcError> {
        WarmLease::discard(self)
    }
}

pub(super) fn handle_run_streaming(
    pool: &WarmPool,
    identity: &WarmOwnerIdentity,
    args: StreamingRun,
    stream: &mut UnixStream,
) {
    let StreamingRun {
        profile,
        egress,
        request_id,
        request,
        accepting_leases,
    } = args;

    if let Err(e) = validate_run_compatibility(identity, profile, &egress, accepting_leases) {
        write_error(stream, &e, Some(request_id));
        return;
    }

    let mut lease = match pool.try_lease() {
        Ok(lease) => lease,
        Err(e) => {
            write_error(stream, &e, Some(request_id));
            return;
        }
    };
    let run_dir = lease.run_dir().display().to_string();
    // BlankVmReset evidence is not wired in v0.1; leases always discard.
    let reset_decision = "Discard".to_owned();
    let discard_reason = "ResetEvidenceUnavailable".to_owned();
    let exit = lease.exec_streaming_with_request_id(request.into(), request_id.clone(), |chunk| {
        control::write_stream_frame(stream, &control::stream_frame_for_chunk(chunk))
    });
    let exit = match exit {
        Ok(exit) => exit,
        Err(e) => {
            let _ = lease.discard();
            write_error(stream, &e, Some(request_id));
            return;
        }
    };
    if let Err(e) = lease.discard() {
        write_error(stream, &e, Some(request_id));
        return;
    }
    let frame = WarmStreamFrame::Exit {
        request_id,
        exit: exit.into(),
        reset_decision,
        discard_reason,
        run_dir,
    };
    if let Err(e) = control::write_stream_frame(stream, &frame) {
        eprintln!("warning: failed to write exit frame to streaming caller: {e}");
    }
}

pub(super) struct StreamingRun {
    pub(super) profile: Option<String>,
    pub(super) egress: String,
    pub(super) request_id: String,
    pub(super) request: ExecRequestJson,
    pub(super) accepting_leases: bool,
}

fn validate_run_compatibility(
    identity: &WarmOwnerIdentity,
    profile: Option<String>,
    egress: &str,
    accepting_leases: bool,
) -> Result<(), FcError> {
    let requested = status::requested_profile(profile);
    if requested != identity.profile {
        return Err(FcError::WarmCompatibilityMismatch {
            field: "profile",
            requested,
            active: identity.profile.clone(),
        });
    }
    if egress != identity.egress {
        return Err(FcError::WarmCompatibilityMismatch {
            field: "egress",
            requested: egress.to_owned(),
            active: identity.egress.clone(),
        });
    }
    if !accepting_leases {
        return Err(FcError::WarmOwnerNotAcceptingLeases);
    }
    Ok(())
}

fn write_error(stream: &mut UnixStream, err: &FcError, request_id: Option<String>) {
    let frame = WarmStreamFrame::Error(WarmErrorResponse::from_error_with_request_id(
        err, request_id,
    ));
    let _ = control::write_stream_frame(stream, &frame);
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use m80_proto::ExecRequest;

    use super::*;

    #[test]
    fn run_request_mismatched_profile_fails_before_lease() {
        let identity = identity("minimal", "outbound");
        let err =
            validate_run_compatibility(&identity, Some("ubuntu".to_owned()), "outbound", true)
                .unwrap_err();

        assert!(err.to_string().contains("warm profile mismatch"));
    }

    #[test]
    fn run_request_empty_pool_returns_pool_empty_without_cold_boot() {
        let response = handle_run_with_pool(
            &FakeEmptyPool { target_ready: 1 },
            &identity("minimal", "none"),
            Some("minimal".to_owned()),
            "none",
            "req-empty-pool".to_owned(),
            ready_probe().into(),
            true,
        );

        match response {
            WarmControlResponse::Error(err) => {
                assert_eq!(err.variant, super::control::WarmErrorKind::PoolEmpty);
                assert_eq!(err.request_id.as_deref(), Some("req-empty-pool"));
                assert_eq!(err.target_ready, Some(1));
            }
            other => panic!("expected PoolEmpty error, got {other:?}"),
        }
    }

    struct FakeEmptyPool {
        target_ready: usize,
    }

    struct FakeLease;

    impl WarmRunPool for FakeEmptyPool {
        type Lease = FakeLease;

        fn try_lease(&self) -> Result<Self::Lease, FcError> {
            Err(FcError::PoolEmpty {
                target_ready: self.target_ready,
            })
        }
    }

    impl WarmRunLease for FakeLease {
        fn run_dir(&self) -> &Path {
            panic!("empty pool must not produce a lease")
        }

        fn exec_with_request_id(
            &mut self,
            _request: ExecRequest,
            _request_id: String,
        ) -> Result<ExecResponse, FcError> {
            panic!("empty pool must not execute")
        }

        fn discard(self) -> Result<(), FcError> {
            panic!("empty pool must not discard")
        }
    }

    fn identity(profile: &str, egress: &str) -> WarmOwnerIdentity {
        WarmOwnerIdentity {
            binary_version: "0.0.0".to_owned(),
            profile: profile.to_owned(),
            egress: egress.to_owned(),
            target_ready: 1,
            pid: 1,
            mode: "foreground".to_owned(),
            socket_path: "/tmp/m80.sock".to_owned(),
            started_at_unix_ms: 0,
        }
    }

    fn ready_probe() -> ExecRequest {
        ExecRequest {
            program: "/bin/true".to_owned(),
            args: Vec::new(),
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(5_000),
            streaming: false,
        }
    }
}
