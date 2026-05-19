use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use m80_proto::{
    read_frame, write_frame, Envelope, PmemMountError, PmemMountRequest, PmemMountResponse,
    PmemMountSpec, PmemMountStatusKind,
};

use super::*;

#[derive(Default)]
struct FakeOps {
    devices: RefCell<HashSet<PathBuf>>,
    mount_table: RefCell<String>,
    waited_for: RefCell<Vec<String>>,
    mount_calls: Cell<usize>,
    dax_after_mount: Cell<bool>,
}

impl FakeOps {
    fn with_device(self, path: &str) -> Self {
        self.devices.borrow_mut().insert(PathBuf::from(path));
        self
    }

    fn with_mount(self, source: &str, target: &str, fstype: &str, options: &str) -> Self {
        self.mount_table
            .borrow_mut()
            .push_str(&format!("{source} {target} {fstype} {options} 0 0\n"));
        self
    }
}

impl PmemMountOps for FakeOps {
    fn device_exists(&self, path: &Path) -> bool {
        self.devices.borrow().contains(path)
    }

    fn wait_for_block(&self, devname: &str, _timeout: Duration) -> Result<(), PmemMountError> {
        self.waited_for.borrow_mut().push(devname.to_owned());
        Ok(())
    }

    fn create_mount_dir(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn mount_erofs_dax(&self, device: &Path, target: &Path) -> io::Result<()> {
        self.mount_calls.set(self.mount_calls.get() + 1);
        let options = if self.dax_after_mount.get() {
            "ro,relatime,dax=always"
        } else {
            "ro,relatime"
        };
        self.mount_table.borrow_mut().push_str(&format!(
            "{} {} erofs {options} 0 0\n",
            device.display(),
            target.display()
        ));
        Ok(())
    }

    fn mount_table(&self) -> io::Result<String> {
        Ok(self.mount_table.borrow().clone())
    }
}

fn spec(device_path: &str, mount_path: &str) -> PmemMountSpec {
    PmemMountSpec {
        device_path: device_path.to_owned(),
        mount_path: mount_path.to_owned(),
        digest_hex: "a".repeat(64),
    }
}

fn request(devices: Vec<PmemMountSpec>) -> PmemMountRequest {
    PmemMountRequest { devices }
}

#[test]
fn mounts_erofs_with_dax_and_reports_mounted() {
    let ops = FakeOps::default().with_device("/dev/pmem0");
    ops.dax_after_mount.set(true);

    let response = mount_devices_with_ops(
        &request(vec![spec("/dev/pmem0", "/opt/m80-layers/rust")]),
        &ops,
    );

    assert_eq!(response.statuses[0].status, PmemMountStatusKind::Mounted);
    assert_eq!(response.statuses[0].error, None);
    assert_eq!(ops.mount_calls.get(), 1);
}

#[test]
fn missing_device_waits_for_pmem_devname_and_fails_closed() {
    let ops = FakeOps::default();

    let response = mount_devices_with_ops(
        &request(vec![spec("/dev/pmem0", "/opt/m80-layers/rust")]),
        &ops,
    );

    assert_eq!(
        response.statuses[0].error,
        Some(PmemMountError::DeviceNotFound)
    );
    assert_eq!(ops.waited_for.borrow().as_slice(), ["pmem0"]);
}

#[test]
fn invalid_device_path_fails_before_waiting() {
    let ops = FakeOps::default();

    let response = mount_devices_with_ops(
        &request(vec![spec("/dev/vda", "/opt/m80-layers/rust")]),
        &ops,
    );

    assert_eq!(
        response.statuses[0].error,
        Some(PmemMountError::InvalidDevicePath)
    );
    assert!(ops.waited_for.borrow().is_empty());
}

#[test]
fn invalid_mount_path_fails_before_mounting() {
    let ops = FakeOps::default().with_device("/dev/pmem0");

    let response = mount_devices_with_ops(&request(vec![spec("/dev/pmem0", "/workspace")]), &ops);

    assert_eq!(
        response.statuses[0].error,
        Some(PmemMountError::InvalidMountPath)
    );
    assert_eq!(ops.mount_calls.get(), 0);
}

#[test]
fn dax_absent_after_mount_fails_closed() {
    let ops = FakeOps::default().with_device("/dev/pmem0");

    let response = mount_devices_with_ops(
        &request(vec![spec("/dev/pmem0", "/opt/m80-layers/rust")]),
        &ops,
    );

    assert_eq!(
        response.statuses[0].error,
        Some(PmemMountError::DaxFlagAbsent)
    );
}

#[test]
fn already_mounted_with_dax_is_noop() {
    let ops = FakeOps::default().with_mount(
        "/dev/pmem0",
        "/opt/m80-layers/rust",
        "erofs",
        "ro,relatime,dax=always",
    );
    ops.devices.borrow_mut().insert(PathBuf::from("/dev/pmem0"));

    let response = mount_devices_with_ops(
        &request(vec![spec("/dev/pmem0", "/opt/m80-layers/rust")]),
        &ops,
    );

    assert_eq!(
        response.statuses[0].status,
        PmemMountStatusKind::AlreadyMounted
    );
    assert_eq!(response.statuses[0].error, None);
    assert_eq!(ops.mount_calls.get(), 0);
}

#[test]
fn handler_decodes_request_and_writes_response() {
    let ops = FakeOps::default().with_device("/dev/pmem0");
    ops.dax_after_mount.set(true);
    let env = Envelope::with_request_id(
        request(vec![spec("/dev/pmem0", "/opt/m80-layers/rust")]),
        "req-pmem".to_owned(),
    );
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    let mut cursor = std::io::Cursor::new(bytes);
    let raw = m80_proto::read_raw_frame(&mut cursor).unwrap();
    let mut out = Vec::new();

    let outcome = handle_pmem_with_ops(raw, &mut out, &ops).unwrap();

    assert_eq!(outcome, ConnectionOutcome::Continue);
    let decoded: Envelope<PmemMountResponse> = read_frame(&mut std::io::Cursor::new(out)).unwrap();
    assert_eq!(decoded.request_id.as_deref(), Some("req-pmem"));
    assert_eq!(
        decoded.payload.statuses[0].status,
        PmemMountStatusKind::Mounted
    );
}

#[test]
fn mount_table_parser_accepts_dax_equals_always() {
    let table = "/dev/pmem0 /opt/m80-layers/rust erofs ro,relatime,dax=always 0 0\n";

    let record = mount_record_from_proc_mounts(table, Path::new("/opt/m80-layers/rust")).unwrap();

    assert_eq!(record.source, PathBuf::from("/dev/pmem0"));
    assert_eq!(record.fstype, "erofs");
    assert!(has_dax_option(&record));
}
