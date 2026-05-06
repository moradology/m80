//! Bead m80-xbn.1.4 — no_egress_reason operator audit field.

mod common;

use m80_image_manifest::{Manifest, DEFAULT_NO_EGRESS_REASON};

#[test]
fn no_egress_reason_round_trips_for_operator_audit() {
    let dir = tempfile::tempdir().unwrap();
    let mut manifest = common::make_artifacts(dir.path());
    manifest.no_egress_reason = Some(DEFAULT_NO_EGRESS_REASON.to_owned());
    let path = dir.path().join("manifest.json");

    manifest.write(&path).unwrap();
    let read_back = Manifest::read(&path).unwrap();

    assert_eq!(
        read_back.no_egress_reason.as_deref(),
        Some(DEFAULT_NO_EGRESS_REASON)
    );
}
