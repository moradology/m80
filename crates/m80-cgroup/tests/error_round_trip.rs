//! Each `CgroupError` variant must Display with a non-empty, sensible message.

use std::io;
use std::path::PathBuf;

use m80_cgroup::CgroupError;

#[test]
fn unsupported_host_mode_display_non_empty() {
    assert!(!CgroupError::UnsupportedHostMode.to_string().is_empty());
}

#[test]
fn controller_not_enabled_display_non_empty() {
    assert!(!CgroupError::ControllerNotEnabled("cpu")
        .to_string()
        .is_empty());
}

#[test]
fn sparse_inherited_file_display_non_empty() {
    assert!(!CgroupError::SparseInheritedFile("cpuset.cpus")
        .to_string()
        .is_empty());
}

#[test]
fn invalid_limit_display_non_empty() {
    assert!(!CgroupError::InvalidLimit {
        field: "io_weight",
        value: "0".into(),
    }
    .to_string()
    .is_empty());
}

#[test]
fn io_error_display_non_empty() {
    let e = CgroupError::Io {
        path: PathBuf::from("/tmp/test"),
        source: io::Error::from(io::ErrorKind::NotFound),
    };
    assert!(!e.to_string().is_empty());
}

#[test]
fn all_variants_display_distinct_messages() {
    let messages = [
        CgroupError::UnsupportedHostMode.to_string(),
        CgroupError::ControllerNotEnabled("cpu").to_string(),
        CgroupError::SparseInheritedFile("cpuset.cpus").to_string(),
        CgroupError::InvalidLimit {
            field: "io_weight",
            value: "0".into(),
        }
        .to_string(),
        CgroupError::Io {
            path: PathBuf::from("/tmp/test"),
            source: io::Error::from(io::ErrorKind::NotFound),
        }
        .to_string(),
    ];
    for i in 0..messages.len() {
        for j in (i + 1)..messages.len() {
            assert_ne!(
                messages[i], messages[j],
                "variants at index {i} and {j} must have distinct display messages"
            );
        }
    }
}
