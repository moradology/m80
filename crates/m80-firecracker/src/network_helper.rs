//! Client for the privileged `m80-net-helper` process.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
#[cfg(not(test))]
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(not(test))]
use std::sync::Weak;

use m80_net_mode::OutboundIntent;
use m80_net_outbound::{
    NetworkHelperRequest, NetworkHelperResponse, NetworkHelperSuccess,
    NETWORK_HELPER_MAX_FRAME_BYTES,
};

use crate::error::{NetworkHelperError, NetworkHelperOperation};

/// Lazily-spawned stdio client for finite privileged network operations.
pub(crate) struct NetworkHelperClient {
    path: PathBuf,
    child: Mutex<Option<NetworkHelperChild>>,
}

#[cfg(not(test))]
static BACKEND_NETWORK_HELPER: Mutex<Option<Weak<NetworkHelperClient>>> = Mutex::new(None);

/// Return the backend helper client and ensure its child is running.
#[cfg(not(test))]
pub(crate) fn backend_network_helper(
    path: &Path,
) -> Result<Arc<NetworkHelperClient>, NetworkHelperError> {
    let mut slot = BACKEND_NETWORK_HELPER
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(helper) = slot.as_ref().and_then(Weak::upgrade) {
        if helper.path != path {
            return Err(NetworkHelperError::PathMismatch {
                active: helper.path.clone(),
                requested: path.to_path_buf(),
            });
        }
        helper.start()?;
        return Ok(helper);
    }

    let helper = Arc::new(NetworkHelperClient::new(path.to_path_buf()));
    helper.start()?;
    *slot = Some(Arc::downgrade(&helper));
    Ok(Arc::clone(&helper))
}

/// Return a test-local backend helper client and ensure its child is running.
#[cfg(test)]
pub(crate) fn backend_network_helper(
    path: &Path,
) -> Result<std::sync::Arc<NetworkHelperClient>, NetworkHelperError> {
    let helper = std::sync::Arc::new(NetworkHelperClient::new(path.to_path_buf()));
    helper.start()?;
    Ok(helper)
}

impl NetworkHelperClient {
    /// Build a client for the preflight-discovered helper executable.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            child: Mutex::new(None),
        }
    }

    /// Start the helper child if this client has not already spawned it.
    pub(crate) fn start(&self) -> Result<(), NetworkHelperError> {
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if guard.is_none() {
            *guard = Some(NetworkHelperChild::spawn(&self.path)?);
        }
        Ok(())
    }

    /// Realize bridge/veth/TAP topology for one outbound VM.
    pub(crate) fn realize_bridge_and_tap(
        &self,
        intent: OutboundIntent,
        vm_id: &str,
        run_root: &Path,
        run_dir: &Path,
    ) -> Result<m80_net_outbound::RealizedNetwork, NetworkHelperError> {
        let operation = NetworkHelperOperation::RealizeBridgeAndTap;
        let success = self.request(
            operation,
            NetworkHelperRequest::RealizeBridgeAndTap {
                intent,
                vm_id: vm_id.to_owned(),
                run_root: run_root.to_path_buf(),
                run_dir: run_dir.to_path_buf(),
            },
        )?;
        match success {
            NetworkHelperSuccess::RealizedNetwork { realized } => Ok(realized),
            NetworkHelperSuccess::Empty => Err(NetworkHelperError::UnexpectedSuccess { operation }),
        }
    }

    /// Apply host NAT/firewall policy for one ready outbound VM.
    pub(crate) fn apply_outbound_nat_policy(
        &self,
        run_dir: &Path,
    ) -> Result<(), NetworkHelperError> {
        self.empty_request(
            NetworkHelperOperation::ApplyOutboundNatPolicy,
            NetworkHelperRequest::ApplyOutboundNatPolicy {
                run_dir: run_dir.to_path_buf(),
            },
        )
    }

    /// Clean one VM's owned outbound network topology and policy.
    pub(crate) fn cleanup_vm(
        &self,
        vm_id: &str,
        run_root: &Path,
    ) -> Result<(), NetworkHelperError> {
        self.empty_request(
            NetworkHelperOperation::CleanupVm,
            NetworkHelperRequest::CleanupVm {
                vm_id: vm_id.to_owned(),
                run_root: run_root.to_path_buf(),
            },
        )
    }

    /// Clean a run-root bridge after the last VM state disappears.
    #[allow(dead_code)]
    pub(crate) fn cleanup_orphan_bridge(&self, run_root: &Path) -> Result<(), NetworkHelperError> {
        self.empty_request(
            NetworkHelperOperation::CleanupOrphanBridge,
            NetworkHelperRequest::CleanupOrphanBridge {
                run_root: run_root.to_path_buf(),
            },
        )
    }

    fn empty_request(
        &self,
        operation: NetworkHelperOperation,
        request: NetworkHelperRequest,
    ) -> Result<(), NetworkHelperError> {
        match self.request(operation, request)? {
            NetworkHelperSuccess::Empty => Ok(()),
            NetworkHelperSuccess::RealizedNetwork { .. } => {
                Err(NetworkHelperError::UnexpectedSuccess { operation })
            }
        }
    }

    fn request(
        &self,
        operation: NetworkHelperOperation,
        request: NetworkHelperRequest,
    ) -> Result<NetworkHelperSuccess, NetworkHelperError> {
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if guard.is_none() {
            drop(guard);
            self.start()?;
            guard = self
                .child
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
        }
        let child = guard
            .as_mut()
            .expect("network helper child must exist after spawn");
        child.request(operation, request)
    }
}

impl std::fmt::Debug for NetworkHelperClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkHelperClient")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

struct NetworkHelperChild {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl NetworkHelperChild {
    fn spawn(path: &Path) -> Result<Self, NetworkHelperError> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|source| NetworkHelperError::Spawn {
                path: path.to_path_buf(),
                source,
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| NetworkHelperError::MissingPipe {
                path: path.to_path_buf(),
                pipe: "stdin",
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NetworkHelperError::MissingPipe {
                path: path.to_path_buf(),
                pipe: "stdout",
            })?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn request(
        &mut self,
        operation: NetworkHelperOperation,
        request: NetworkHelperRequest,
    ) -> Result<NetworkHelperSuccess, NetworkHelperError> {
        let frame = encode_request(operation, &request)?;
        self.stdin
            .write_all(&frame)
            .map_err(|source| NetworkHelperError::Io { operation, source })?;
        self.stdin
            .flush()
            .map_err(|source| NetworkHelperError::Io { operation, source })?;

        let response = read_response(operation, &mut self.stdout)?;
        match response {
            NetworkHelperResponse::Ok { success } => Ok(success),
            NetworkHelperResponse::Err { failure } => Err(NetworkHelperError::OperationFailed {
                operation,
                kind: failure.kind,
                detail: failure.detail,
            }),
        }
    }
}

impl Drop for NetworkHelperChild {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            if error.kind() != std::io::ErrorKind::InvalidInput {
                tracing::warn!(error = %error, "failed to kill network helper child");
            }
        }
        if let Err(error) = self.child.wait() {
            tracing::warn!(error = %error, "failed to wait for network helper child");
        }
    }
}

fn encode_request(
    operation: NetworkHelperOperation,
    request: &NetworkHelperRequest,
) -> Result<Vec<u8>, NetworkHelperError> {
    let mut frame =
        serde_json::to_vec(request).map_err(|source| NetworkHelperError::RequestEncode {
            operation,
            detail: source.to_string(),
        })?;
    if frame.len() > NETWORK_HELPER_MAX_FRAME_BYTES {
        return Err(NetworkHelperError::RequestEncode {
            operation,
            detail: format!(
                "network helper request exceeds {} bytes",
                NETWORK_HELPER_MAX_FRAME_BYTES
            ),
        });
    }
    frame.push(b'\n');
    Ok(frame)
}

fn read_response<R>(
    operation: NetworkHelperOperation,
    reader: &mut R,
) -> Result<NetworkHelperResponse, NetworkHelperError>
where
    R: BufRead,
{
    let mut frame = Vec::new();
    match read_bounded_response_frame(operation, reader, &mut frame)? {
        NetworkHelperFrameRead::Eof => {
            return Err(NetworkHelperError::Eof { operation });
        }
        NetworkHelperFrameRead::Frame => {}
        NetworkHelperFrameRead::Oversized => {
            return Err(NetworkHelperError::OversizedResponse {
                operation,
                limit: NETWORK_HELPER_MAX_FRAME_BYTES,
            });
        }
    }
    serde_json::from_slice(&frame)
        .map_err(|source| NetworkHelperError::ResponseDecode { operation, source })
}

enum NetworkHelperFrameRead {
    Eof,
    Frame,
    Oversized,
}

fn read_bounded_response_frame<R>(
    operation: NetworkHelperOperation,
    reader: &mut R,
    frame: &mut Vec<u8>,
) -> Result<NetworkHelperFrameRead, NetworkHelperError>
where
    R: BufRead,
{
    frame.clear();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|source| NetworkHelperError::Io { operation, source })?;
        if available.is_empty() {
            if frame.is_empty() {
                return Ok(NetworkHelperFrameRead::Eof);
            }
            trim_frame_cr(frame);
            return Ok(NetworkHelperFrameRead::Frame);
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if frame.len() + newline > NETWORK_HELPER_MAX_FRAME_BYTES {
                reader.consume(newline + 1);
                frame.clear();
                return Ok(NetworkHelperFrameRead::Oversized);
            }
            frame.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            trim_frame_cr(frame);
            return Ok(NetworkHelperFrameRead::Frame);
        }
        let available_len = available.len();
        if frame.len() + available_len > NETWORK_HELPER_MAX_FRAME_BYTES {
            reader.consume(available_len);
            discard_until_response_newline(operation, reader)?;
            frame.clear();
            return Ok(NetworkHelperFrameRead::Oversized);
        }
        frame.extend_from_slice(available);
        reader.consume(available_len);
    }
}

fn discard_until_response_newline<R>(
    operation: NetworkHelperOperation,
    reader: &mut R,
) -> Result<(), NetworkHelperError>
where
    R: BufRead,
{
    loop {
        let available = reader
            .fill_buf()
            .map_err(|source| NetworkHelperError::Io { operation, source })?;
        if available.is_empty() {
            return Ok(());
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            reader.consume(newline + 1);
            return Ok(());
        }
        let available_len = available.len();
        reader.consume(available_len);
    }
}

fn trim_frame_cr(frame: &mut Vec<u8>) {
    while matches!(frame.last(), Some(b'\r')) {
        frame.pop();
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use m80_net_outbound::NetworkHelperFailureKind;

    use super::*;

    fn write_helper_script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("helper.sh");
        std::fs::write(&path, body).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn realize_bridge_and_tap_uses_helper_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("requests.log");
        let script = format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "{}"
  printf '%s\n' '{{"status":"ok","success":{{"kind":"realized_network","realized":{{"bridge_name":"br-test","tap_name":"tap-test","vmm_netns_path":"/run/netns/m80-test","guest_ipv4":"172.16.0.2","guest_mac":"02:00:00:00:00:01","bridge_cidr":"172.16.0.0/24"}}}}}}'
done
"#,
            log.display()
        );
        let helper = NetworkHelperClient::new(write_helper_script(dir.path(), &script));

        let realized = helper
            .realize_bridge_and_tap(
                OutboundIntent {
                    exceptions: Vec::new(),
                },
                "vm-a",
                Path::new("/run/m80"),
                Path::new("/run/m80/vm-a"),
            )
            .unwrap();

        assert_eq!(realized.tap_name, "tap-test");
        let requests = std::fs::read_to_string(log).unwrap();
        assert!(requests.contains(r#""op":"realize_bridge_and_tap""#));
        assert!(requests.contains(r#""vm_id":"vm-a""#));
    }

    #[test]
    fn helper_operation_failure_stays_typed() {
        let dir = tempfile::tempdir().unwrap();
        let script = r#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"err","failure":{"kind":"operation_failed","detail":"synthetic helper denial"}}'
done
"#;
        let helper = NetworkHelperClient::new(write_helper_script(dir.path(), script));

        let err = helper
            .cleanup_vm("vm-a", Path::new("/run/m80"))
            .unwrap_err();

        assert!(matches!(
            err,
            NetworkHelperError::OperationFailed {
                operation: NetworkHelperOperation::CleanupVm,
                kind: NetworkHelperFailureKind::OperationFailed,
                ref detail,
            } if detail == "synthetic helper denial"
        ));
    }

    #[test]
    fn oversized_helper_response_is_rejected_before_decode() {
        let mut input = vec![b' '; NETWORK_HELPER_MAX_FRAME_BYTES + 1];
        input.push(b'\n');

        let err = read_response(NetworkHelperOperation::CleanupVm, &mut &input[..]).unwrap_err();

        assert!(matches!(
            err,
            NetworkHelperError::OversizedResponse {
                operation: NetworkHelperOperation::CleanupVm,
                limit: NETWORK_HELPER_MAX_FRAME_BYTES,
            }
        ));
    }
}
