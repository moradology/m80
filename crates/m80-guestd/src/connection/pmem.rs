//! Guest pmem mount request handler.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use m80_proto::{
    PmemMountError, PmemMountRequest, PmemMountResponse, PmemMountSpec, PmemMountStatus,
    PmemMountStatusKind, RawEnvelope, PAYLOAD_KIND_PMEM_MOUNT_REQUEST,
};
use nix::mount::{mount, MsFlags};

use super::{protocol_log, write_payload_frame, ConnectionOutcome};
use crate::guest_log::GuestLogPhase;
use crate::uevent::{spawn_netlink_listener, BlockDeviceMatcher, UeventRegistry, UeventWaitError};

const PMEM_WAIT: Duration = Duration::from_millis(250);
const DEV_PMEM_PREFIX: &str = "/dev/pmem";

static UEVENT_REGISTRY: OnceLock<Arc<UeventRegistry>> = OnceLock::new();
static UEVENT_REGISTRY_INIT: Mutex<()> = Mutex::new(());

pub(super) fn is_pmem_kind(kind: &str) -> bool {
    kind == PAYLOAD_KIND_PMEM_MOUNT_REQUEST
}

pub(super) fn handle_pmem<R, W>(
    raw: RawEnvelope,
    _reader: R,
    writer: &mut W,
) -> anyhow::Result<ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let request_id = raw.request_id.clone();
    let kind = raw.kind.clone();
    let env = raw.decode::<PmemMountRequest>().inspect_err(|e| {
        protocol_log::warn_proto_error(
            GuestLogPhase::Exec,
            request_id.as_deref(),
            Some(kind.as_str()),
            e,
        );
    })?;
    let response = mount_devices(&env.payload);
    write_payload_frame(writer, &env.request_id, response)?;
    Ok(ConnectionOutcome::Continue)
}

fn prod_device_exists(path: &Path) -> bool {
    path.exists()
}

fn prod_wait_for_block(devname: &str, timeout: Duration) -> Result<(), PmemMountError> {
    let registry = uevent_registry()?;
    registry
        .wait_for(&BlockDeviceMatcher::devname(devname), timeout)
        .map(|_| ())
        .map_err(|e| match e {
            UeventWaitError::Timeout => PmemMountError::DeviceNotFound,
        })
}

fn prod_create_mount_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

fn prod_mount_erofs_dax(device: &Path, target: &Path) -> io::Result<()> {
    mount(
        Some(device),
        target,
        Some("erofs"),
        MsFlags::MS_RDONLY | MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
        Some("dax=always"),
    )
    .map_err(io::Error::other)
}

fn prod_mount_table() -> io::Result<String> {
    std::fs::read_to_string("/proc/mounts")
}

fn mount_devices(request: &PmemMountRequest) -> PmemMountResponse {
    PmemMountResponse {
        statuses: request.devices.iter().map(mount_one).collect(),
    }
}

fn mount_one(spec: &PmemMountSpec) -> PmemMountStatus {
    let device_path = match wait_for_device(spec) {
        Ok(device_path) => device_path,
        Err(error) => return failed(spec, error),
    };
    let mount_path = match validate_mount_path(&spec.mount_path) {
        Ok(path) => path,
        Err(error) => return failed(spec, error),
    };

    let before = match prod_mount_table() {
        Ok(table) => mount_record_from_proc_mounts(&table, &mount_path),
        Err(_) => return failed(spec, PmemMountError::Io),
    };
    if let Some(record) = before {
        if record.source == device_path && record.fstype == "erofs" {
            if has_dax_option(&record) {
                return status(spec, PmemMountStatusKind::AlreadyMounted, None);
            }
            return failed(spec, PmemMountError::DaxFlagAbsent);
        }
        return failed(spec, PmemMountError::MountFailed);
    }

    if prod_create_mount_dir(&mount_path).is_err() {
        return failed(spec, PmemMountError::Io);
    }
    if let Err(err) = prod_mount_erofs_dax(&device_path, &mount_path) {
        crate::guest_log::error(
            GuestLogPhase::Exec,
            None,
            format!(
                "pmem_mount_failed device={} target={} error={err}",
                device_path.display(),
                mount_path.display()
            ),
        );
        return failed(spec, PmemMountError::MountFailed);
    }
    match prod_mount_table()
        .ok()
        .and_then(|table| mount_record_from_proc_mounts(&table, &mount_path))
    {
        Some(record) if record.source == device_path && record.fstype == "erofs" => {
            if has_dax_option(&record) {
                status(spec, PmemMountStatusKind::Mounted, None)
            } else {
                failed(spec, PmemMountError::DaxFlagAbsent)
            }
        }
        _ => failed(spec, PmemMountError::DaxFlagAbsent),
    }
}

fn wait_for_device(spec: &PmemMountSpec) -> Result<PathBuf, PmemMountError> {
    let devname = validate_device_path(&spec.device_path)?;
    let device_path = PathBuf::from(&spec.device_path);
    if prod_device_exists(&device_path) {
        return Ok(device_path);
    }
    prod_wait_for_block(devname, PMEM_WAIT)?;
    if prod_device_exists(&device_path) {
        Ok(device_path)
    } else {
        Err(PmemMountError::DeviceNotFound)
    }
}

fn validate_device_path(path: &str) -> Result<&str, PmemMountError> {
    let suffix = path
        .strip_prefix(DEV_PMEM_PREFIX)
        .ok_or(PmemMountError::InvalidDevicePath)?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PmemMountError::InvalidDevicePath);
    }
    Ok(&path["/dev/".len()..])
}

fn validate_mount_path(path: &str) -> Result<PathBuf, PmemMountError> {
    let raw_components = path.split('/').collect::<Vec<_>>();
    if raw_components.len() != 4
        || raw_components[0] != ""
        || raw_components[1] != "opt"
        || raw_components[2] != "m80-layers"
    {
        return Err(PmemMountError::InvalidMountPath);
    }

    let name = raw_components[3];
    if name.is_empty()
        || name.len() > 64
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(PmemMountError::InvalidMountPath);
    }
    Ok(PathBuf::from(path))
}

fn status(
    spec: &PmemMountSpec,
    status: PmemMountStatusKind,
    error: Option<PmemMountError>,
) -> PmemMountStatus {
    PmemMountStatus {
        device_path: spec.device_path.clone(),
        mount_path: spec.mount_path.clone(),
        status,
        error,
    }
}

fn failed(spec: &PmemMountSpec, error: PmemMountError) -> PmemMountStatus {
    status(spec, PmemMountStatusKind::Failed, Some(error))
}

fn uevent_registry() -> Result<Arc<UeventRegistry>, PmemMountError> {
    if let Some(registry) = UEVENT_REGISTRY.get() {
        return Ok(Arc::clone(registry));
    }
    let _guard = UEVENT_REGISTRY_INIT.lock().unwrap();
    if let Some(registry) = UEVENT_REGISTRY.get() {
        return Ok(Arc::clone(registry));
    }
    let registry = Arc::new(UeventRegistry::default());
    spawn_netlink_listener(Arc::clone(&registry)).map_err(|_| PmemMountError::Io)?;
    let _ = UEVENT_REGISTRY.set(Arc::clone(&registry));
    Ok(Arc::clone(UEVENT_REGISTRY.get().unwrap()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MountRecord {
    source: PathBuf,
    fstype: String,
    options: Vec<String>,
}

fn mount_record_from_proc_mounts(input: &str, target: &Path) -> Option<MountRecord> {
    let target = target.to_str()?;
    for line in input.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 4 || fields[1] != target {
            continue;
        }
        return Some(MountRecord {
            source: PathBuf::from(fields[0]),
            fstype: fields[2].to_owned(),
            options: fields[3].split(',').map(str::to_owned).collect(),
        });
    }
    None
}

fn has_dax_option(record: &MountRecord) -> bool {
    record
        .options
        .iter()
        .any(|option| option == "dax" || option.starts_with("dax="))
}

#[cfg(test)]
pub(crate) trait PmemMountOps {
    fn device_exists(&self, path: &Path) -> bool;
    fn wait_for_block(&self, devname: &str, timeout: Duration) -> Result<(), PmemMountError>;
    fn create_mount_dir(&self, path: &Path) -> io::Result<()>;
    fn mount_erofs_dax(&self, device: &Path, target: &Path) -> io::Result<()>;
    fn mount_table(&self) -> io::Result<String>;
}

#[cfg(test)]
fn mount_devices_with_ops(
    request: &PmemMountRequest,
    ops: &impl PmemMountOps,
) -> PmemMountResponse {
    PmemMountResponse {
        statuses: request
            .devices
            .iter()
            .map(|spec| mount_one_with_ops(spec, ops))
            .collect(),
    }
}

#[cfg(test)]
fn mount_one_with_ops(spec: &PmemMountSpec, ops: &impl PmemMountOps) -> PmemMountStatus {
    let device_path = match wait_for_device_with_ops(spec, ops) {
        Ok(device_path) => device_path,
        Err(error) => return failed(spec, error),
    };
    let mount_path = match validate_mount_path(&spec.mount_path) {
        Ok(path) => path,
        Err(error) => return failed(spec, error),
    };

    let before = match ops.mount_table() {
        Ok(table) => mount_record_from_proc_mounts(&table, &mount_path),
        Err(_) => return failed(spec, PmemMountError::Io),
    };
    if let Some(record) = before {
        if record.source == device_path && record.fstype == "erofs" {
            if has_dax_option(&record) {
                return status(spec, PmemMountStatusKind::AlreadyMounted, None);
            }
            return failed(spec, PmemMountError::DaxFlagAbsent);
        }
        return failed(spec, PmemMountError::MountFailed);
    }

    if ops.create_mount_dir(&mount_path).is_err() {
        return failed(spec, PmemMountError::Io);
    }
    if ops.mount_erofs_dax(&device_path, &mount_path).is_err() {
        return failed(spec, PmemMountError::MountFailed);
    }
    match ops
        .mount_table()
        .ok()
        .and_then(|table| mount_record_from_proc_mounts(&table, &mount_path))
    {
        Some(record) if record.source == device_path && record.fstype == "erofs" => {
            if has_dax_option(&record) {
                status(spec, PmemMountStatusKind::Mounted, None)
            } else {
                failed(spec, PmemMountError::DaxFlagAbsent)
            }
        }
        _ => failed(spec, PmemMountError::DaxFlagAbsent),
    }
}

#[cfg(test)]
fn wait_for_device_with_ops(
    spec: &PmemMountSpec,
    ops: &impl PmemMountOps,
) -> Result<PathBuf, PmemMountError> {
    let devname = validate_device_path(&spec.device_path)?;
    let device_path = PathBuf::from(&spec.device_path);
    if ops.device_exists(&device_path) {
        return Ok(device_path);
    }
    ops.wait_for_block(devname, PMEM_WAIT)?;
    if ops.device_exists(&device_path) {
        Ok(device_path)
    } else {
        Err(PmemMountError::DeviceNotFound)
    }
}

#[cfg(test)]
fn handle_pmem_with_ops<W>(
    raw: RawEnvelope,
    writer: &mut W,
    ops: &impl PmemMountOps,
) -> anyhow::Result<ConnectionOutcome>
where
    W: Write,
{
    let request_id = raw.request_id.clone();
    let kind = raw.kind.clone();
    let env = raw.decode::<PmemMountRequest>().inspect_err(|e| {
        protocol_log::warn_proto_error(
            GuestLogPhase::Exec,
            request_id.as_deref(),
            Some(kind.as_str()),
            e,
        );
    })?;
    let response = mount_devices_with_ops(&env.payload, ops);
    write_payload_frame(writer, &env.request_id, response)?;
    Ok(ConnectionOutcome::Continue)
}

#[cfg(test)]
mod tests;
