//! Each `CgroupError` variant must Display with a non-empty, sensible message.

use std::io;
use std::path::PathBuf;

use m80_cgroup::CgroupError;

#[test]
fn all_variants_display_distinct_messages() {
    let variants: Vec<(&str, CgroupError)> = vec![
        ("UnsupportedHostMode", CgroupError::UnsupportedHostMode),
        (
            "ControllerNotEnabled",
            CgroupError::ControllerNotEnabled("cpu"),
        ),
        (
            "Io",
            CgroupError::Io {
                path: PathBuf::from("/tmp/test"),
                source: io::Error::from(io::ErrorKind::NotFound),
            },
        ),
    ];

    let messages: Vec<String> = variants.iter().map(|(_, e)| e.to_string()).collect();
    // All messages non-empty.
    for (name, msg) in variants.iter().zip(messages.iter()) {
        assert!(!msg.is_empty(), "{} display must not be empty", name.0);
    }
    // All messages distinct.
    for i in 0..messages.len() {
        for j in (i + 1)..messages.len() {
            assert_ne!(
                messages[i], messages[j],
                "variants {} and {} must have distinct display messages",
                variants[i].0, variants[j].0
            );
        }
    }
}
