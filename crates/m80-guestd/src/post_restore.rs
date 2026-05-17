//! Host-driven post-restore hook executor.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use m80_proto::{
    write_frame, Envelope, HookError, HookKindWire, HookResultWire, HookStatus,
    PostRestoreHookRequest, PostRestoreHookResponse, RawEnvelope,
};

use crate::connection::ConnectionOutcome;
use crate::guest_log::{self, GuestLogPhase};

const RANDOM_SEED_BYTES: usize = 32;
const MACHINE_ID_BYTES: usize = 16;

/// Handle one post-restore hook request frame.
pub(crate) fn handle_post_restore<W: Write>(
    raw: RawEnvelope,
    writer: &mut W,
) -> anyhow::Result<ConnectionOutcome> {
    let request_id = raw.request_id.clone();
    let env = match raw.decode::<PostRestoreHookRequest>() {
        Ok(env) => env,
        Err(e) => {
            guest_log::warn(
                GuestLogPhase::Exec,
                request_id.as_deref(),
                format!("post-restore hook decode failed: {e}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };
    let response = run_post_restore_hooks(env.payload);
    let out = match env.request_id {
        Some(id) => Envelope::with_request_id(response, id),
        None => Envelope::new(response),
    };
    write_frame(writer, &out)?;
    writer.flush()?;
    Ok(ConnectionOutcome::Continue)
}

#[cfg(test)]
fn handle_post_restore_with_ops<W: Write>(
    raw: RawEnvelope,
    writer: &mut W,
    paths: &PostRestorePaths,
    ops: &impl KernelOps,
) -> anyhow::Result<ConnectionOutcome> {
    let request_id = raw.request_id.clone();
    let env = match raw.decode::<PostRestoreHookRequest>() {
        Ok(env) => env,
        Err(e) => {
            guest_log::warn(
                GuestLogPhase::Exec,
                request_id.as_deref(),
                format!("post-restore hook decode failed: {e}"),
            );
            return Ok(ConnectionOutcome::Continue);
        }
    };
    let response = run_post_restore_hooks_with_ops(&env.payload, paths, ops);
    let out = match env.request_id {
        Some(id) => Envelope::with_request_id(response, id),
        None => Envelope::new(response),
    };
    write_frame(writer, &out)?;
    writer.flush()?;
    Ok(ConnectionOutcome::Continue)
}

/// Run the current-profile post-restore hook sequence.
#[must_use]
pub(crate) fn run_post_restore_hooks(request: PostRestoreHookRequest) -> PostRestoreHookResponse {
    run_post_restore_hooks_with_ops(&request, &PostRestorePaths::guest_root(), &ProdKernelOps)
}

fn run_post_restore_hooks_with_ops(
    request: &PostRestoreHookRequest,
    paths: &PostRestorePaths,
    ops: &impl KernelOps,
) -> PostRestoreHookResponse {
    let mut results = Vec::new();
    if let Err(error) = mix_nonce_and_reseed(&request.restore_nonce, paths, ops) {
        results.push(failed(HookKindWire::ReseedSystemdRandomSeed, error));
        return PostRestoreHookResponse { results };
    }

    for hook in &request.hooks {
        match run_one_hook(hook, paths, ops) {
            Ok(()) => results.push(succeeded(hook.clone())),
            Err(error) => {
                results.push(failed(hook.clone(), error));
                break;
            }
        }
    }
    PostRestoreHookResponse { results }
}

fn mix_nonce_and_reseed(
    restore_nonce: &[u8; 32],
    paths: &PostRestorePaths,
    ops: &impl KernelOps,
) -> Result<(), HookError> {
    ops.mix_restore_nonce(&paths.urandom, restore_nonce)
        .map_err(|_| HookError::ReseedFailed)?;
    ops.reseed_crng(&paths.urandom)
        .map_err(|_| HookError::ReseedFailed)
}

fn run_one_hook(
    hook: &HookKindWire,
    paths: &PostRestorePaths,
    ops: &impl KernelOps,
) -> Result<(), HookError> {
    match hook {
        HookKindWire::ReseedSystemdRandomSeed => reseed_systemd_random_seed(paths, ops),
        HookKindWire::RegenMachineId => regen_machine_id(paths, ops),
        HookKindWire::SetHostname { hostname } => set_hostname(hostname.as_str(), paths, ops),
    }
}

fn reseed_systemd_random_seed(
    paths: &PostRestorePaths,
    ops: &impl KernelOps,
) -> Result<(), HookError> {
    if !paths.random_seed.exists() {
        return Ok(());
    }
    let mut bytes = [0_u8; RANDOM_SEED_BYTES];
    ops.fill_random(&mut bytes)
        .map_err(|_| HookError::RandomSeedWriteFailed)?;
    std::fs::write(&paths.random_seed, bytes).map_err(|_| HookError::RandomSeedWriteFailed)
}

fn regen_machine_id(paths: &PostRestorePaths, ops: &impl KernelOps) -> Result<(), HookError> {
    let mut bytes = [0_u8; MACHINE_ID_BYTES];
    ops.fill_random(&mut bytes)
        .map_err(|_| HookError::MachineIdWriteFailed)?;
    std::fs::write(&paths.machine_id, format!("{}\n", hex_lower(&bytes)))
        .map_err(|_| HookError::MachineIdWriteFailed)
}

fn set_hostname(
    hostname: &str,
    paths: &PostRestorePaths,
    ops: &impl KernelOps,
) -> Result<(), HookError> {
    HookKindWire::set_hostname(hostname.to_owned()).map_err(|_| HookError::InvalidHostname)?;
    ops.set_hostname(hostname)
        .map_err(|err| HookError::HostnameSyscallFailed {
            errno: err.raw_os_error().unwrap_or(0),
        })?;
    std::fs::write(&paths.hostname, format!("{hostname}\n"))
        .map_err(|_| HookError::HostnameWriteFailed)
}

fn succeeded(kind: HookKindWire) -> HookResultWire {
    HookResultWire {
        kind,
        status: HookStatus::Succeeded,
        error: None,
    }
}

fn failed(kind: HookKindWire, error: HookError) -> HookResultWire {
    HookResultWire {
        kind,
        status: HookStatus::Failed,
        error: Some(error),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

struct PostRestorePaths {
    urandom: PathBuf,
    machine_id: PathBuf,
    hostname: PathBuf,
    random_seed: PathBuf,
}

impl PostRestorePaths {
    fn guest_root() -> Self {
        Self {
            urandom: PathBuf::from("/dev/urandom"),
            machine_id: PathBuf::from("/etc/machine-id"),
            hostname: PathBuf::from("/etc/hostname"),
            random_seed: PathBuf::from("/var/lib/systemd/random-seed"),
        }
    }
}

trait KernelOps {
    fn mix_restore_nonce(&self, urandom: &Path, restore_nonce: &[u8; 32]) -> io::Result<()>;
    fn reseed_crng(&self, urandom: &Path) -> io::Result<()>;
    fn fill_random(&self, bytes: &mut [u8]) -> io::Result<()>;
    fn set_hostname(&self, hostname: &str) -> io::Result<()>;
}

struct ProdKernelOps;

impl KernelOps for ProdKernelOps {
    fn mix_restore_nonce(&self, urandom: &Path, restore_nonce: &[u8; 32]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).open(urandom)?;
        file.write_all(restore_nonce)?;
        file.flush()
    }

    fn reseed_crng(&self, urandom: &Path) -> io::Result<()> {
        let file = File::open(urandom)?;
        m80_guest_kernel::rndreseedcrng(&file)
    }

    fn fill_random(&self, bytes: &mut [u8]) -> io::Result<()> {
        m80_guest_kernel::fill_random(bytes)
    }

    fn set_hostname(&self, hostname: &str) -> io::Result<()> {
        m80_guest_kernel::set_hostname(hostname)
    }
}

#[cfg(test)]
mod tests;
