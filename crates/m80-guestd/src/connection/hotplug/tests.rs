use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_proto::{
    read_frame, write_frame, DriveHotplugError, DriveMountRequest, DriveMountResponse,
    DriveMountSpec, DriveMountStatusKind, Envelope,
};

use super::*;

#[derive(Default)]
struct FakeOps {
    devices: RefCell<HashSet<PathBuf>>,
    mountpoints: RefCell<HashMap<PathBuf, PathBuf>>,
    identities: RefCell<HashMap<PathBuf, Vec<u8>>>,
    waited_for: RefCell<Vec<String>>,
    mount_calls: Cell<usize>,
}

impl FakeOps {
    fn with_device(self, path: &str) -> Self {
        self.devices.borrow_mut().insert(PathBuf::from(path));
        self
    }

    fn with_mount(self, target: &str, source: &str) -> Self {
        self.mountpoints
            .borrow_mut()
            .insert(PathBuf::from(target), PathBuf::from(source));
        self
    }

    fn with_identity(self, path: &str, bytes: &[u8]) -> Self {
        self.identities
            .borrow_mut()
            .insert(PathBuf::from(path), bytes.to_vec());
        self
    }
}

impl MountOps for FakeOps {
    fn device_exists(&self, path: &Path) -> bool {
        self.devices.borrow().contains(path)
    }

    fn wait_for_block(&self, devname: &str, _timeout: Duration) -> Result<(), DriveHotplugError> {
        self.waited_for.borrow_mut().push(devname.to_owned());
        Ok(())
    }

    fn create_mount_dir(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn mount_ext4(&self, device: &Path, target: &Path) -> io::Result<()> {
        self.mount_calls.set(self.mount_calls.get() + 1);
        self.mountpoints
            .borrow_mut()
            .insert(target.to_path_buf(), device.to_path_buf());
        Ok(())
    }

    fn read_identity(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.identities
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn mount_source(&self, target: &Path) -> io::Result<Option<PathBuf>> {
        Ok(self.mountpoints.borrow().get(target).cloned())
    }
}

fn request(devices: Vec<DriveMountSpec>) -> DriveMountRequest {
    DriveMountRequest { devices }
}

fn spec(
    drive_id: &str,
    device_path: &str,
    mount_path: &str,
    identity_path: Option<&str>,
) -> DriveMountSpec {
    DriveMountSpec {
        drive_id: drive_id.to_owned(),
        device_path: device_path.to_owned(),
        mount_path: mount_path.to_owned(),
        identity_path: identity_path.map(str::to_owned),
    }
}

#[test]
fn missing_device_waits_for_requested_devname() {
    let ops = FakeOps::default();
    let req = request(vec![spec("hotplug_slot_0", "/dev/vdd", "/tenant", None)]);

    let response = mount_devices(&req, &ops);

    assert_eq!(
        response.statuses[0].error,
        Some(DriveHotplugError::DeviceNotFound)
    );
    assert_eq!(ops.waited_for.borrow().as_slice(), ["vdd"]);
}

#[test]
fn non_dev_device_path_fails_closed() {
    let ops = FakeOps::default();
    let req = request(vec![spec("hotplug_slot_0", "vdd", "/tenant", None)]);

    let response = mount_devices(&req, &ops);

    assert_eq!(
        response.statuses[0].error,
        Some(DriveHotplugError::DeviceNotFound)
    );
    assert!(ops.waited_for.borrow().is_empty());
}

#[test]
fn mount_request_returns_mounted_and_identity_bytes() {
    let ops = FakeOps::default()
        .with_mount("/workspace", "/dev/vdc")
        .with_device("/dev/vdd")
        .with_identity("/tenant.id", b"tenant-a");
    let response = mount_devices(
        &request(vec![spec(
            "hotplug_slot_0",
            "/dev/vdd",
            "/tenant",
            Some("/tenant.id"),
        )]),
        &ops,
    );

    assert_eq!(response.statuses.len(), 1);
    assert_eq!(response.statuses[0].status, DriveMountStatusKind::Mounted);
    assert_eq!(response.statuses[0].error, None);
    assert_eq!(response.identities.len(), 1);
    assert_eq!(response.identities[0].bytes, b"tenant-a");
    assert_eq!(ops.mount_calls.get(), 1);
}

#[test]
fn partial_success_reports_each_device_status() {
    let ops = FakeOps::default()
        .with_mount("/workspace", "/dev/vdc")
        .with_device("/dev/vdd");
    let response = mount_devices(
        &request(vec![
            spec("hotplug_slot_0", "/dev/vdd", "/tenant-a", None),
            spec("hotplug_slot_1", "/dev/vde", "/tenant-b", None),
        ]),
        &ops,
    );

    assert_eq!(response.statuses.len(), 2);
    assert_eq!(response.statuses[0].status, DriveMountStatusKind::Mounted);
    assert_eq!(response.statuses[0].error, None);
    assert_eq!(response.statuses[1].status, DriveMountStatusKind::Failed);
    assert_eq!(
        response.statuses[1].error,
        Some(DriveHotplugError::DeviceNotFound)
    );
    assert_eq!(ops.mount_calls.get(), 1);
}

#[test]
fn already_mounted_matching_device_is_noop() {
    let ops = FakeOps::default()
        .with_mount("/workspace", "/dev/vdc")
        .with_device("/dev/vdd")
        .with_mount("/tenant", "/dev/vdd");
    let response = mount_devices(
        &request(vec![spec("hotplug_slot_0", "/dev/vdd", "/tenant", None)]),
        &ops,
    );

    assert_eq!(response.statuses.len(), 1);
    assert_eq!(
        response.statuses[0].status,
        DriveMountStatusKind::AlreadyMounted
    );
    assert_eq!(response.statuses[0].error, None);
    assert_eq!(ops.mount_calls.get(), 0);
}

#[test]
fn handler_decodes_request_and_writes_response() {
    let ops = FakeOps::default()
        .with_mount("/workspace", "/dev/vdc")
        .with_device("/dev/vdd");
    let env = Envelope::with_request_id(
        request(vec![spec("hotplug_slot_0", "/dev/vdd", "/tenant", None)]),
        "req-hotplug".to_owned(),
    );
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    let mut cursor = std::io::Cursor::new(bytes);
    let raw = m80_proto::read_raw_frame(&mut cursor).unwrap();
    let mut out = Vec::new();

    let outcome = handle_hotplug_with_ops(raw, &mut out, &ops).unwrap();

    assert_eq!(outcome, ConnectionOutcome::Continue);
    let decoded: Envelope<DriveMountResponse> = read_frame(&mut std::io::Cursor::new(out)).unwrap();
    assert_eq!(decoded.request_id.as_deref(), Some("req-hotplug"));
    assert_eq!(
        decoded.payload.statuses[0].status,
        DriveMountStatusKind::Mounted
    );
}

#[test]
fn mountinfo_source_parser_returns_matching_source() {
    let input = "35 24 8:4 / /tenant rw,relatime - ext4 /dev/vdd rw\n";

    let source = mount_source_from_info(input, Path::new("/tenant")).unwrap();

    assert_eq!(source, Some(PathBuf::from("/dev/vdd")));
}
