//! Guest drive hotplug mount request handler.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use m80_proto::{
    DriveDetachRequest, DriveDetachResponse, DriveDetachSpec, DriveDetachStatus,
    DriveDetachStatusKind, DriveHotplugError, DriveMountRequest, DriveMountResponse,
    DriveMountSpec, DriveMountStatus, DriveMountStatusKind, RawEnvelope, TenantIdentityReport,
    PAYLOAD_KIND_DRIVE_DETACH_REQUEST, PAYLOAD_KIND_DRIVE_MOUNT_REQUEST,
};
use nix::mount::{mount, umount2, MntFlags, MsFlags};

use super::{protocol_log, write_payload_frame, ConnectionOutcome};
use crate::guest_log::GuestLogPhase;
use crate::uevent::{spawn_netlink_listener, BlockDeviceMatcher, UeventRegistry, UeventWaitError};

const HOTPLUG_WAIT: Duration = Duration::from_millis(250);
const DEV_PREFIX: &str = "/dev/";

static UEVENT_REGISTRY: OnceLock<Arc<UeventRegistry>> = OnceLock::new();
static UEVENT_REGISTRY_INIT: Mutex<()> = Mutex::new(());

pub(super) fn is_hotplug_kind(kind: &str) -> bool {
    matches!(
        kind,
        PAYLOAD_KIND_DRIVE_MOUNT_REQUEST | PAYLOAD_KIND_DRIVE_DETACH_REQUEST
    )
}

pub(super) fn handle_hotplug<R, W>(
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
    match kind.as_str() {
        PAYLOAD_KIND_DRIVE_MOUNT_REQUEST => {
            let env = raw.decode::<DriveMountRequest>().inspect_err(|e| {
                protocol_log::warn_proto_error(
                    GuestLogPhase::Exec,
                    request_id.as_deref(),
                    Some(kind.as_str()),
                    e,
                );
            })?;
            let response = mount_devices(&env.payload);
            write_payload_frame(writer, &env.request_id, response)?;
        }
        PAYLOAD_KIND_DRIVE_DETACH_REQUEST => {
            let env = raw.decode::<DriveDetachRequest>().inspect_err(|e| {
                protocol_log::warn_proto_error(
                    GuestLogPhase::Exec,
                    request_id.as_deref(),
                    Some(kind.as_str()),
                    e,
                );
            })?;
            let response = detach_devices(&env.payload);
            write_payload_frame(writer, &env.request_id, response)?;
        }
        _ => unreachable!("is_hotplug_kind filters dispatch"),
    }
    Ok(ConnectionOutcome::Continue)
}

// ── Production free functions ─────────────────────────────────────────────────

fn prod_device_exists(path: &Path) -> bool {
    path.exists()
}

fn prod_wait_for_block(devname: &str, timeout: Duration) -> Result<(), DriveHotplugError> {
    let registry = uevent_registry()?;
    registry
        .wait_for(&BlockDeviceMatcher::devname(devname), timeout)
        .map(|_| ())
        .map_err(|e| match e {
            UeventWaitError::Timeout => DriveHotplugError::Timeout,
        })
}

fn prod_create_mount_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

fn prod_mount_ext4(device: &Path, target: &Path) -> io::Result<()> {
    mount(
        Some(device),
        target,
        Some("ext4"),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(io::Error::other)
}

fn prod_sync_all() -> io::Result<()> {
    nix::unistd::sync();
    Ok(())
}

fn prod_unmount(target: &Path) -> io::Result<()> {
    umount2(target, MntFlags::MNT_DETACH).map_err(io::Error::other)
}

fn prod_read_identity(path: &Path) -> io::Result<Vec<u8>> {
    std::fs::read(path)
}

fn prod_mount_source(target: &Path) -> io::Result<Option<PathBuf>> {
    mount_source_from_info(&std::fs::read_to_string("/proc/self/mountinfo")?, target)
}

// ── Production dispatch ───────────────────────────────────────────────────────

fn mount_devices(request: &DriveMountRequest) -> DriveMountResponse {
    let mut statuses = Vec::with_capacity(request.devices.len());
    let mut identities = Vec::new();

    for spec in &request.devices {
        let outcome = mount_one(spec);
        statuses.push(outcome.status);
        if let Some(identity) = outcome.identity {
            identities.push(identity);
        }
    }

    DriveMountResponse {
        statuses,
        identities,
    }
}

fn detach_devices(request: &DriveDetachRequest) -> DriveDetachResponse {
    DriveDetachResponse {
        statuses: request
            .devices
            .iter()
            .map(|spec| detach_one(spec))
            .collect(),
    }
}

struct MountOutcome {
    status: DriveMountStatus,
    identity: Option<TenantIdentityReport>,
}

fn wait_for_device(spec: &DriveMountSpec) -> Result<PathBuf, DriveHotplugError> {
    let devname = spec
        .device_path
        .strip_prefix(DEV_PREFIX)
        .ok_or(DriveHotplugError::DeviceNotFound)?;
    let device_path = PathBuf::from(&spec.device_path);
    if prod_device_exists(&device_path) {
        return Ok(device_path);
    }
    prod_wait_for_block(devname, HOTPLUG_WAIT)?;
    if prod_device_exists(&device_path) {
        Ok(device_path)
    } else {
        Err(DriveHotplugError::DeviceNotFound)
    }
}

fn mount_one(spec: &DriveMountSpec) -> MountOutcome {
    let device_path = match wait_for_device(spec) {
        Ok(device_path) => device_path,
        Err(error) => return failed(spec, error),
    };

    let already_mounted = match prod_mount_source(Path::new(&spec.mount_path)) {
        Ok(Some(source)) if source == device_path => true,
        Ok(Some(_)) => return failed(spec, DriveHotplugError::MountFailed),
        Ok(None) => false,
        Err(_) => return failed(spec, DriveHotplugError::Io),
    };

    let status = if already_mounted {
        DriveMountStatusKind::AlreadyMounted
    } else {
        if prod_create_mount_dir(Path::new(&spec.mount_path)).is_err() {
            return failed(spec, DriveHotplugError::Io);
        }
        if prod_mount_ext4(&device_path, Path::new(&spec.mount_path)).is_err() {
            return failed(spec, DriveHotplugError::MountFailed);
        }
        DriveMountStatusKind::Mounted
    };

    let identity = match &spec.identity_path {
        Some(path) => match prod_read_identity(Path::new(path)) {
            Ok(bytes) => Some(TenantIdentityReport {
                drive_id: spec.drive_id.clone(),
                path: path.clone(),
                bytes,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return failed(spec, DriveHotplugError::IdentityMissing);
            }
            Err(_) => return failed(spec, DriveHotplugError::IdentityReadFailed),
        },
        None => None,
    };

    MountOutcome {
        status: DriveMountStatus {
            drive_id: spec.drive_id.clone(),
            mount_path: spec.mount_path.clone(),
            status,
            error: None,
        },
        identity,
    }
}

fn failed(spec: &DriveMountSpec, error: DriveHotplugError) -> MountOutcome {
    MountOutcome {
        status: DriveMountStatus {
            drive_id: spec.drive_id.clone(),
            mount_path: spec.mount_path.clone(),
            status: DriveMountStatusKind::Failed,
            error: Some(error),
        },
        identity: None,
    }
}

fn detach_one(spec: &DriveDetachSpec) -> DriveDetachStatus {
    let mount_path = Path::new(&spec.mount_path);
    match prod_mount_source(mount_path) {
        Ok(None) => detach_status(spec, DriveDetachStatusKind::NotMounted, None),
        Ok(Some(_)) => {
            if prod_sync_all().is_err() {
                return detach_status(
                    spec,
                    DriveDetachStatusKind::Failed,
                    Some(DriveHotplugError::Io),
                );
            }
            if prod_unmount(mount_path).is_err() {
                return detach_status(
                    spec,
                    DriveDetachStatusKind::Failed,
                    Some(DriveHotplugError::UnmountFailed),
                );
            }
            detach_status(spec, DriveDetachStatusKind::Detached, None)
        }
        Err(_) => detach_status(
            spec,
            DriveDetachStatusKind::Failed,
            Some(DriveHotplugError::Io),
        ),
    }
}

fn detach_status(
    spec: &DriveDetachSpec,
    status: DriveDetachStatusKind,
    error: Option<DriveHotplugError>,
) -> DriveDetachStatus {
    DriveDetachStatus {
        drive_id: spec.drive_id.clone(),
        mount_path: spec.mount_path.clone(),
        status,
        error,
    }
}

fn uevent_registry() -> Result<Arc<UeventRegistry>, DriveHotplugError> {
    if let Some(registry) = UEVENT_REGISTRY.get() {
        return Ok(Arc::clone(registry));
    }
    let _guard = UEVENT_REGISTRY_INIT.lock().unwrap();
    if let Some(registry) = UEVENT_REGISTRY.get() {
        return Ok(Arc::clone(registry));
    }
    let registry = Arc::new(UeventRegistry::default());
    spawn_netlink_listener(Arc::clone(&registry)).map_err(|_| DriveHotplugError::Io)?;
    let _ = UEVENT_REGISTRY.set(Arc::clone(&registry));
    Ok(Arc::clone(UEVENT_REGISTRY.get().unwrap()))
}

fn mount_source_from_info(input: &str, target: &Path) -> io::Result<Option<PathBuf>> {
    let target = target
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "mount target is not utf-8"))?;
    for line in input.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[4] != target {
            continue;
        }
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            continue;
        };
        if let Some(source) = fields.get(separator + 2) {
            return Ok(Some(PathBuf::from(source)));
        }
    }
    Ok(None)
}

// ── Test injection: trait + fake impl ────────────────────────────────────────

/// In tests, `MountOps` replaces the prod free functions for controllable fakes.
#[cfg(test)]
pub(crate) trait MountOps {
    fn device_exists(&self, path: &Path) -> bool;
    fn wait_for_block(&self, devname: &str, timeout: Duration) -> Result<(), DriveHotplugError>;
    fn create_mount_dir(&self, path: &Path) -> io::Result<()>;
    fn mount_ext4(&self, device: &Path, target: &Path) -> io::Result<()>;
    fn sync_all(&self) -> io::Result<()>;
    fn unmount(&self, target: &Path) -> io::Result<()>;
    fn read_identity(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn mount_source(&self, target: &Path) -> io::Result<Option<PathBuf>>;
}

#[cfg(test)]
fn wait_for_device_with_ops(
    spec: &DriveMountSpec,
    ops: &impl MountOps,
) -> Result<PathBuf, DriveHotplugError> {
    let devname = spec
        .device_path
        .strip_prefix(DEV_PREFIX)
        .ok_or(DriveHotplugError::DeviceNotFound)?;
    let device_path = PathBuf::from(&spec.device_path);
    if ops.device_exists(&device_path) {
        return Ok(device_path);
    }
    ops.wait_for_block(devname, HOTPLUG_WAIT)?;
    if ops.device_exists(&device_path) {
        Ok(device_path)
    } else {
        Err(DriveHotplugError::DeviceNotFound)
    }
}

#[cfg(test)]
fn mount_one_with_ops(spec: &DriveMountSpec, ops: &impl MountOps) -> MountOutcome {
    let device_path = match wait_for_device_with_ops(spec, ops) {
        Ok(device_path) => device_path,
        Err(error) => return failed(spec, error),
    };

    let already_mounted = match ops.mount_source(Path::new(&spec.mount_path)) {
        Ok(Some(source)) if source == device_path => true,
        Ok(Some(_)) => return failed(spec, DriveHotplugError::MountFailed),
        Ok(None) => false,
        Err(_) => return failed(spec, DriveHotplugError::Io),
    };

    let status = if already_mounted {
        DriveMountStatusKind::AlreadyMounted
    } else {
        if ops.create_mount_dir(Path::new(&spec.mount_path)).is_err() {
            return failed(spec, DriveHotplugError::Io);
        }
        if ops
            .mount_ext4(&device_path, Path::new(&spec.mount_path))
            .is_err()
        {
            return failed(spec, DriveHotplugError::MountFailed);
        }
        DriveMountStatusKind::Mounted
    };

    let identity = match &spec.identity_path {
        Some(path) => match ops.read_identity(Path::new(path)) {
            Ok(bytes) => Some(TenantIdentityReport {
                drive_id: spec.drive_id.clone(),
                path: path.clone(),
                bytes,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return failed(spec, DriveHotplugError::IdentityMissing);
            }
            Err(_) => return failed(spec, DriveHotplugError::IdentityReadFailed),
        },
        None => None,
    };

    MountOutcome {
        status: DriveMountStatus {
            drive_id: spec.drive_id.clone(),
            mount_path: spec.mount_path.clone(),
            status,
            error: None,
        },
        identity,
    }
}

#[cfg(test)]
fn detach_one_with_ops(spec: &DriveDetachSpec, ops: &impl MountOps) -> DriveDetachStatus {
    let mount_path = Path::new(&spec.mount_path);
    match ops.mount_source(mount_path) {
        Ok(None) => detach_status(spec, DriveDetachStatusKind::NotMounted, None),
        Ok(Some(_)) => {
            if ops.sync_all().is_err() {
                return detach_status(
                    spec,
                    DriveDetachStatusKind::Failed,
                    Some(DriveHotplugError::Io),
                );
            }
            if ops.unmount(mount_path).is_err() {
                return detach_status(
                    spec,
                    DriveDetachStatusKind::Failed,
                    Some(DriveHotplugError::UnmountFailed),
                );
            }
            detach_status(spec, DriveDetachStatusKind::Detached, None)
        }
        Err(_) => detach_status(
            spec,
            DriveDetachStatusKind::Failed,
            Some(DriveHotplugError::Io),
        ),
    }
}

#[cfg(test)]
fn mount_devices_with_ops(request: &DriveMountRequest, ops: &impl MountOps) -> DriveMountResponse {
    let mut statuses = Vec::with_capacity(request.devices.len());
    let mut identities = Vec::new();

    for spec in &request.devices {
        let outcome = mount_one_with_ops(spec, ops);
        statuses.push(outcome.status);
        if let Some(identity) = outcome.identity {
            identities.push(identity);
        }
    }

    DriveMountResponse {
        statuses,
        identities,
    }
}

#[cfg(test)]
fn detach_devices_with_ops(
    request: &DriveDetachRequest,
    ops: &impl MountOps,
) -> DriveDetachResponse {
    DriveDetachResponse {
        statuses: request
            .devices
            .iter()
            .map(|spec| detach_one_with_ops(spec, ops))
            .collect(),
    }
}

#[cfg(test)]
fn handle_hotplug_with_ops<W>(
    raw: RawEnvelope,
    writer: &mut W,
    ops: &impl MountOps,
) -> anyhow::Result<ConnectionOutcome>
where
    W: Write,
{
    let request_id = raw.request_id.clone();
    let kind = raw.kind.clone();
    let env = raw.decode::<DriveMountRequest>().inspect_err(|e| {
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
fn handle_detach_with_ops<W>(
    raw: RawEnvelope,
    writer: &mut W,
    ops: &impl MountOps,
) -> anyhow::Result<ConnectionOutcome>
where
    W: Write,
{
    let request_id = raw.request_id.clone();
    let kind = raw.kind.clone();
    let env = raw.decode::<DriveDetachRequest>().inspect_err(|e| {
        protocol_log::warn_proto_error(
            GuestLogPhase::Exec,
            request_id.as_deref(),
            Some(kind.as_str()),
            e,
        );
    })?;

    let response = detach_devices_with_ops(&env.payload, ops);
    write_payload_frame(writer, &env.request_id, response)?;
    Ok(ConnectionOutcome::Continue)
}

#[cfg(test)]
mod tests;
