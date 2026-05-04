//! Round-trip `ChangeSet` and `Rejection` through `serde_json`.

use std::path::PathBuf;

use m80_storage::{ChangeSet, Rejection, RejectionReason};

fn make_changeset() -> ChangeSet {
    ChangeSet {
        staged: vec![PathBuf::from("dir/file.txt"), PathBuf::from("another.bin")],
        rejected: vec![
            Rejection {
                path: PathBuf::from("link.txt"),
                reason: RejectionReason::Symlink,
            },
            Rejection {
                path: PathBuf::from("device"),
                reason: RejectionReason::SpecialFile,
            },
            Rejection {
                path: PathBuf::from("weird"),
                reason: RejectionReason::Other("unsupported file type".into()),
            },
        ],
        total_bytes: 4096,
    }
}

#[test]
fn changeset_round_trips_through_json() {
    let cs = make_changeset();
    let json = serde_json::to_string(&cs).expect("serialize");
    let cs2: ChangeSet = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(cs.staged, cs2.staged);
    assert_eq!(cs.total_bytes, cs2.total_bytes);
    assert_eq!(cs.rejected.len(), cs2.rejected.len());
}

#[test]
fn rejection_reason_symlink_round_trips() {
    let r = Rejection {
        path: PathBuf::from("link"),
        reason: RejectionReason::Symlink,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("symlink"), "tag must appear: {json}");
    let r2: Rejection = serde_json::from_str(&json).unwrap();
    assert_eq!(r2.path, r.path);
    assert!(matches!(r2.reason, RejectionReason::Symlink));
}

#[test]
fn rejection_reason_special_file_round_trips() {
    let r = Rejection {
        path: PathBuf::from("dev"),
        reason: RejectionReason::SpecialFile,
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains("special_file"), "tag must appear: {json}");
    let r2: Rejection = serde_json::from_str(&json).unwrap();
    assert!(matches!(r2.reason, RejectionReason::SpecialFile));
}

#[test]
fn rejection_reason_other_preserves_message() {
    let r = Rejection {
        path: PathBuf::from("x"),
        reason: RejectionReason::Other("some reason".into()),
    };
    let json = serde_json::to_string(&r).unwrap();
    let r2: Rejection = serde_json::from_str(&json).unwrap();
    match r2.reason {
        RejectionReason::Other(msg) => assert_eq!(msg, "some reason"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn empty_changeset_round_trips() {
    let cs = ChangeSet {
        staged: vec![],
        rejected: vec![],
        total_bytes: 0,
    };
    let json = serde_json::to_string(&cs).unwrap();
    let cs2: ChangeSet = serde_json::from_str(&json).unwrap();
    assert!(cs2.staged.is_empty());
    assert!(cs2.rejected.is_empty());
    assert_eq!(cs2.total_bytes, 0);
}
