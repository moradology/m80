mod control;
mod owner;
mod run;
mod status;

use std::io::{stdout, Write as _};

use m80_firecracker::{ExecChunk, ExecRequest, ExecResponse, FcError};

use crate::args::{EgressMode, WarmAction};
use crate::errors;
use crate::json;

use super::proto_json::{ExecRequestJson, ExecResponseJson};
use control::{WarmControlRequest, WarmControlResponse};

pub(super) fn cmd_warm(action: WarmAction, json: bool) -> anyhow::Result<i32> {
    let code = match action {
        WarmAction::Enable(args) => {
            if args.system {
                super::render_not_implemented(
                    "`m80 warm enable --system` is reserved for service packaging",
                    json,
                )
            } else if !args.foreground {
                let e = FcError::config_other(
                    "m80 warm enable requires --foreground in the first owner implementation"
                        .to_owned(),
                );
                errors::render_error(&e, json)
            } else if args.size == 0 {
                let e = FcError::config_other("m80 warm enable --size must be greater than zero");
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
            &FcError::config_other("warm owner returned status for run request"),
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
                    return errors::render_error(&FcError::Io(e), false);
                }
            }
            Ok(control::WarmStreamFrame::Stderr { seq, bytes }) => {
                let chunk = ExecChunk::Stderr { seq, bytes };
                if let Err(e) = super::run_stream::copy_guest_chunk(chunk, &mut stdout, &mut stderr)
                {
                    return errors::render_error(&FcError::Io(e), false);
                }
            }
            Ok(control::WarmStreamFrame::Exit { exit, .. }) => {
                if let Err(e) = stdout.flush().and_then(|_| stderr.flush()) {
                    return errors::render_error(&FcError::Io(e), false);
                }
                let exit = m80_firecracker::ExecExit::from(exit);
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
            let e = FcError::config_other("warm owner returned run result for status request");
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
            &FcError::config_other("warm owner returned run result for lifecycle request"),
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
            &FcError::config_other("warm owner returned run result for disable request"),
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
            return errors::render_error(&FcError::Io(e), json_mode);
        }
        if let Err(e) = stderr
            .write_all(&response_native.stderr)
            .and_then(|_| stderr.flush())
        {
            return errors::render_error(&FcError::Io(e), json_mode);
        }
    }
    super::run_stream::process_exit_code(response_native.status, response_native.exit_code, None)
}

fn render_owner_error(err: control::WarmErrorResponse, json_mode: bool) -> i32 {
    let exit_code = err.exit_code;
    if json_mode {
        let env = errors::ErrorEnvelope {
            request_id: err.request_id,
            variant: err.variant.as_str(),
            detail: err.detail,
            exit_code,
        };
        eprintln!("{}", json::to_pretty(&env));
    } else if let Some(request_id) = err.request_id {
        eprintln!("error: [{request_id}] {}", err.detail);
    } else {
        eprintln!("error: {}", err.detail);
    }
    exit_code
}
