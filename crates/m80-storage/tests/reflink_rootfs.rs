//! Concurrent rootfs overlay-template clone regressions.

mod common;

use std::fmt;
use std::fs::OpenOptions;
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use m80_storage::Rootfs;
use sha2::{Digest as _, Sha256};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

use common::reflink_helpers::{assert_reflink_always_is_unsupported, assert_sparse_file};

const OVERLAY_SIZE: u64 = 64 * 1024 * 1024;

static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn concurrent_fill_produces_eight_independent_overlays() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempfile::tempdir_in(".").unwrap();
    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();

    let template = dir.path().join(".rootfs-overlay-template-v1-67108864.ext4");
    let base = Arc::new(base);
    let mut handles = Vec::new();
    for idx in 0..8 {
        let base = Arc::clone(&base);
        let run_dir = dir.path().join(format!("vm-{idx}"));
        std::fs::create_dir(&run_dir).unwrap();
        let overlay = run_dir.join("rootfs.overlay.ext4");
        handles.push(thread::spawn(move || {
            Rootfs::prepare(&base, &overlay, OVERLAY_SIZE).unwrap();
            overlay
        }));
    }

    let overlays: Vec<PathBuf> = handles
        .into_iter()
        .map(|handle| handle.join().expect("prepare thread must not panic"))
        .collect();

    assert_eq!(template_count(dir.path()), 1);
    assert!(!dir
        .path()
        .join(".rootfs-overlay-template-v1-67108864.lock")
        .exists());
    assert_sparse_file(&template, OVERLAY_SIZE);
    for overlay in &overlays {
        assert!(overlay.exists(), "{} exists", overlay.display());
        assert_sparse_file(overlay, OVERLAY_SIZE);
    }

    let template_before = sha256(&template);
    for (idx, overlay) in overlays.iter().enumerate() {
        write_marker(overlay, idx);
    }
    assert_eq!(sha256(&template), template_before);
}

#[test]
fn concurrent_fill_does_not_double_emit_byte_copy_event() {
    let _guard = TEST_LOCK.lock().unwrap();
    let dir = tempfile::Builder::new()
        .prefix("m80-rootfs-byte-copy-event-")
        .tempdir_in("/dev/shm")
        .expect("/dev/shm tmpfs must be available for non-reflink fallback test");
    assert_reflink_always_is_unsupported(dir.path());

    let base = dir.path().join("base.ext4");
    std::fs::write(&base, b"fake-base").unwrap();
    let base = Arc::new(base);
    let collector = EventCollector::default();
    let dispatch = tracing::Dispatch::new(collector.clone());

    let mut handles = Vec::new();
    for idx in 0..4 {
        let base = Arc::clone(&base);
        let dispatch = dispatch.clone();
        let run_dir = dir.path().join(format!("vm-{idx}"));
        std::fs::create_dir(&run_dir).unwrap();
        let overlay = run_dir.join("rootfs.overlay.ext4");
        handles.push(thread::spawn(move || {
            tracing::dispatcher::with_default(&dispatch, || {
                Rootfs::prepare(&base, &overlay, OVERLAY_SIZE).unwrap();
            });
        }));
    }

    for handle in handles {
        handle.join().expect("prepare thread must not panic");
    }

    let events = collector.events();
    let byte_copy_events = events
        .iter()
        .filter(|event| {
            event.target == "m80_storage::rootfs"
                && event.field_equals("mode", "byte_copy")
                && event.field_equals("fs_kind", "tmpfs")
        })
        .count();
    assert_eq!(byte_copy_events, 1, "events: {events:#?}");
}

fn template_count(run_root: &Path) -> usize {
    std::fs::read_dir(run_root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".rootfs-overlay-template-v1-") && name.ends_with(".ext4")
        })
        .count()
}

fn write_marker(path: &Path, idx: usize) {
    let mut file = OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(1024 * 1024 + idx as u64 * 4096))
        .unwrap();
    file.write_all(format!("m80 overlay marker {idx}\n").as_bytes())
        .unwrap();
}

fn sha256(path: &Path) -> [u8; 32] {
    let bytes = std::fs::read(path).unwrap();
    Sha256::digest(bytes).into()
}

#[derive(Clone, Default)]
struct EventCollector {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl EventCollector {
    fn events(&self) -> Vec<CapturedEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl Subscriber for EventCollector {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.events.lock().unwrap().push(CapturedEvent {
            target: event.metadata().target().to_owned(),
            fields: visitor.fields,
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Clone, Debug)]
struct CapturedEvent {
    target: String,
    fields: Vec<(String, String)>,
}

impl CapturedEvent {
    fn field_equals(&self, name: &str, value: &str) -> bool {
        self.fields
            .iter()
            .any(|(field, actual)| field == name && actual == value)
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: Vec<(String, String)>,
}

impl FieldVisitor {
    fn record_value(&mut self, field: &Field, value: impl fmt::Display) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_value(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_value(field, value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_value(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_value(field, value);
    }
}
