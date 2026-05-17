//! Minimal public construction chain for a one-command VM run.

use std::sync::Arc;

use m80_firecracker::{Backend, BackendConfig, FcError, SandboxConfig};
use m80_proto::ExecRequest;

fn main() -> Result<(), FcError> {
    let discovery = m80_preflight::run()?;
    let backend_config = BackendConfig::builder(discovery).build();
    let backend = Arc::new(Backend::new(backend_config)?);

    let sandbox = backend.admit(SandboxConfig {
        vm_id: Some("minimal-launch".to_owned()),
        ..SandboxConfig::default()
    })?;
    let mut running = sandbox.launch()?;

    let _response = running.exec(ExecRequest {
        program: "/bin/true".to_owned(),
        args: Vec::new(),
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(30_000),
        streaming: false,
    })?;

    let stopped = running.stop()?;
    stopped.delete()?;
    Ok(())
}
