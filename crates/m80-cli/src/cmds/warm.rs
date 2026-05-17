mod control;
mod owner;
mod run;
mod status;

use std::io::{stdout, Write as _};

use m80_firecracker::{ConfigError, ExecChunk, FcError};
use m80_proto::{ExecExit, ExecRequest, ExecResponse};

use crate::args::{EgressMode, WarmAction};
use crate::errors;
use crate::json;

use super::proto_json::{ExecRequestJson, ExecResponseJson};
use control::{WarmControlRequest, WarmControlResponse};

pub(super) fn cmd_warm(action: WarmAction, json: bool) -> anyhow::Result<i32> {
    let code = match action {
        WarmAction::Enable(args) => {
            if args.size == 0 {
                let e = FcError::Config(ConfigError::InvalidValue {
                    field: "size",
                    reason: "m80 warm enable --size must be greater than zero".to_owned(),
                });
                errors::render_error(&e, json)
            } else {
                owner::run_foreground(args.profile, args.egress, args.size, json)
            }
        }
        WarmAction::Status { profile } => render_status(profile, json),
        WarmAction::Drain => send_owner_lifecycle_request(WarmControlRequest::Drain, json),
        WarmAction::Disable => disable_owner(json),
    };
    Ok(code)
}

pub(super) fn cmd_run_warm(
    profile: Option<String>,
    egress: EgressMode,
    req: ExecRequest,
    json_mode: bool,
) -> i32 {
    let request_id = match crate::request_id::current() {
        Some(request_id) => request_id,
        None => crate::request_id::new(),
    };
    if !json_mode {
        return render_warm_streaming_run(profile, egress, request_id, req);
    }
    match control::send_to_owner(WarmControlRequest::Run {
        profile,
        egress: status::egress_label(egress).to_owned(),
        request_id,
        request: ExecRequestJson::from(req),
    }) {
        Ok(WarmControlResponse::Run(result)) => render_warm_run(result.response, json_mode),
        Ok(WarmControlResponse::Error(err)) => render_owner_error(err, json_mode),
        Ok(WarmControlResponse::Status(_)) => errors::render_error(
            &FcError::UnexpectedWarmResponse {
                request: "run",
                response: "status",
            },
            json_mode,
        ),
        Err(e) => errors::render_error(&e, json_mode),
    }
}

fn render_warm_streaming_run(
    profile: Option<String>,
    egress: EgressMode,
    request_id: String,
    req: ExecRequest,
) -> i32 {
    let mut reader = match control::send_stream_request(WarmControlRequest::RunStream {
        profile,
        egress: status::egress_label(egress).to_owned(),
        request_id,
        request: ExecRequestJson::from(req),
    }) {
        Ok(reader) => reader,
        Err(e) => return errors::render_error(&e, false),
    };
    let mut stdout = stdout().lock();
    let mut stderr = std::io::stderr().lock();
    loop {
        match control::read_stream_frame(&mut reader) {
            Ok(control::WarmStreamFrame::Stdout { seq, bytes }) => {
                let chunk = ExecChunk::Stdout { seq, bytes };
                if let Err(e) = super::run_stream::copy_guest_chunk(chunk, &mut stdout, &mut stderr)
                {
                    return errors::render_error(
                        &errors::host_io("write warm streamed stdout", e),
                        false,
                    );
                }
            }
            Ok(control::WarmStreamFrame::Stderr { seq, bytes }) => {
                let chunk = ExecChunk::Stderr { seq, bytes };
                if let Err(e) = super::run_stream::copy_guest_chunk(chunk, &mut stdout, &mut stderr)
                {
                    return errors::render_error(
                        &errors::host_io("write warm streamed stderr", e),
                        false,
                    );
                }
            }
            Ok(control::WarmStreamFrame::Exit { exit, .. }) => {
                if let Err(e) = stdout.flush().and_then(|_| stderr.flush()) {
                    return errors::render_error(
                        &errors::host_io("flush warm streamed output", e),
                        false,
                    );
                }
                let exit = ExecExit::from(exit);
                return super::run_stream::process_exit_code(exit.status, exit.exit_code, None);
            }
            Ok(control::WarmStreamFrame::Error(err)) => return render_owner_error(err, false),
            Err(e) => return errors::render_error(&e, false),
        }
    }
}

fn render_status(profile: Option<String>, json_mode: bool) -> i32 {
    let response = control::send_to_owner(WarmControlRequest::Status {
        profile: profile.clone(),
    });
    let status = match response {
        Ok(WarmControlResponse::Status(status)) => status,
        Ok(WarmControlResponse::Error(err)) => return render_owner_error(err, json_mode),
        Ok(WarmControlResponse::Run(_)) => {
            let e = FcError::UnexpectedWarmResponse {
                request: "status",
                response: "run result",
            };
            return errors::render_error(&e, json_mode);
        }
        Err(_) => status::unavailable(profile),
    };
    if json_mode {
        println!("{}", json::to_pretty(&status));
    } else {
        print!("{}", status::format_human(&status));
    }
    0
}

fn send_owner_lifecycle_request(req: WarmControlRequest, json_mode: bool) -> i32 {
    match control::send_to_owner(req) {
        Ok(WarmControlResponse::Status(status)) => {
            if json_mode {
                println!("{}", json::to_pretty(&status));
            } else {
                print!("{}", status::format_human(&status));
            }
            0
        }
        Ok(WarmControlResponse::Error(err)) => render_owner_error(err, json_mode),
        Ok(WarmControlResponse::Run(_)) => errors::render_error(
            &FcError::UnexpectedWarmResponse {
                request: "lifecycle",
                response: "run result",
            },
            json_mode,
        ),
        Err(e) => errors::render_error(&e, json_mode),
    }
}

fn disable_owner(json_mode: bool) -> i32 {
    match control::send_to_owner(WarmControlRequest::Disable) {
        Ok(response) => send_owner_lifecycle_response(response, json_mode),
        Err(_) => {
            let status = status::disabled(None);
            if let Err(e) = status::remove_owner_state() {
                return errors::render_error(&e, json_mode);
            }
            if json_mode {
                println!("{}", json::to_pretty(&status));
            } else {
                print!("{}", status::format_human(&status));
            }
            0
        }
    }
}

fn send_owner_lifecycle_response(response: WarmControlResponse, json_mode: bool) -> i32 {
    match response {
        WarmControlResponse::Status(status) => {
            if json_mode {
                println!("{}", json::to_pretty(&status));
            } else {
                print!("{}", status::format_human(&status));
            }
            0
        }
        WarmControlResponse::Error(err) => render_owner_error(err, json_mode),
        WarmControlResponse::Run(_) => errors::render_error(
            &FcError::UnexpectedWarmResponse {
                request: "disable",
                response: "run result",
            },
            json_mode,
        ),
    }
}

fn render_warm_run(response: ExecResponseJson, json_mode: bool) -> i32 {
    let response_native = ExecResponse::from(response.clone());
    if json_mode {
        println!("{}", json::to_pretty(&response));
    } else {
        let mut stdout = stdout().lock();
        let mut stderr = std::io::stderr().lock();
        if let Err(e) = stdout
            .write_all(&response_native.stdout)
            .and_then(|_| stdout.flush())
        {
            return errors::render_error(&errors::host_io("write warm stdout", e), json_mode);
        }
        if let Err(e) = stderr
            .write_all(&response_native.stderr)
            .and_then(|_| stderr.flush())
        {
            return errors::render_error(&errors::host_io("write warm stderr", e), json_mode);
        }
    }
    super::run_stream::process_exit_code(response_native.status, response_native.exit_code, None)
}

fn render_owner_error(err: control::WarmErrorResponse, json_mode: bool) -> i32 {
    let exit_code = err.exit_code;
    if json_mode {
        // Set the request_id scope so json::to_pretty picks it up in the outer
        // wrapper; do not duplicate it inside ErrorEnvelope.
        let _scope = err
            .request_id
            .as_deref()
            .map(|id| crate::request_id::set(id.to_owned()));
        let env = errors::ErrorEnvelope {
            variant: err.variant.as_str(),
            detail: err.detail,
            exit_code,
            target_ready: err.target_ready,
        };
        eprintln!("{}", json::to_pretty(&env));
    } else if let Some(request_id) = err.request_id {
        eprintln!("error: [{request_id}] {}", err.detail);
    } else {
        eprintln!("error: {}", err.detail);
    }
    exit_code
}
