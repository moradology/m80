//! Each PreflightError variant must expose a non-empty hint via `hint()`.

use caps::Capability;
use m80_image_manifest::ManifestError;
use m80_preflight::PreflightError;

fn assert_hint(err: &PreflightError) {
    let msg = err.to_string();
    assert!(!msg.is_empty(), "Display for {err:?} must not be empty");
    // Hint is now a separate method, not embedded in Display.
    let hint = err.hint();
    assert!(!hint.is_empty(), "hint() for {err:?} must not be empty");
    // Display must NOT contain embedded hint text (regression guard).
    assert!(
        !msg.contains("hint:"),
        "Display for {err:?} must not embed hint text; got:\n{msg}"
    );
}

#[test]
fn unsupported_host_platform_has_hint() {
    let err = PreflightError::UnsupportedHostPlatform {
        actual: "darwin".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("darwin"));
}

#[test]
fn kvm_unavailable_has_hint() {
    let err = PreflightError::KvmUnavailable {
        path: "/dev/kvm".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("/dev/kvm"));
}

#[test]
fn kvm_not_writable_has_hint() {
    let err = PreflightError::KvmNotWritable {
        path: "/dev/kvm".into(),
    };
    assert_hint(&err);
    assert!(err.to_string().contains("/dev/kvm"));
}

#[test]
fn kernel_modules_missing_has_hint() {
    assert_hint(&PreflightError::KernelModulesMissing {
        missing: vec!["tap".into()],
    });
}

#[test]
fn privilege_unavailable_has_hint() {
    assert_hint(&PreflightError::PrivilegeUnavailable {
        missing_caps: vec![Capability::CAP_NET_ADMIN],
    });
}

#[test]
fn firecracker_binary_not_found_has_hint() {
    assert_hint(&PreflightError::FirecrackerBinaryNotFound);
}

#[test]
fn firecracker_version_mismatch_has_hint() {
    assert_hint(&PreflightError::FirecrackerVersionMismatch {
        expected: "v1.15.1".into(),
        actual: "v1.14.0".into(),
    });
}

#[test]
fn jailer_binary_not_found_has_hint() {
    assert_hint(&PreflightError::JailerBinaryNotFound);
}

#[test]
fn non_absolute_path_has_hint() {
    assert_hint(&PreflightError::NonAbsolutePath {
        kind: "rootfs".into(),
        path: "rootfs.ext4".into(),
    });
}

#[test]
fn kernel_not_found_has_hint() {
    assert_hint(&PreflightError::KernelNotFound);
}

#[test]
fn rootfs_not_found_has_hint() {
    assert_hint(&PreflightError::RootfsNotFound);
}

#[test]
fn manifest_error_has_hint() {
    let inner = ManifestError::UnsupportedSchemaVersion(99);
    assert_hint(&PreflightError::Manifest(inner));
}

#[test]
fn run_root_unavailable_has_hint() {
    assert_hint(&PreflightError::RunRootUnavailable {
        reason: "directory does not exist: /var/run/m80".into(),
    });
}

#[test]
fn storage_helper_missing_has_hint() {
    assert_hint(&PreflightError::StorageHelperMissing("mkfs.ext4".into()));
}

#[test]
fn io_error_has_hint() {
    let io_err = std::io::Error::other("disk full");
    assert_hint(&PreflightError::Io(io_err));
}

#[test]
fn capability_read_has_hint() {
    use caps::errors::CapsError;
    let err = PreflightError::CapabilityRead(CapsError::from("kernel returned EINVAL"));
    assert_hint(&err);
    assert!(err.to_string().contains("capability read failed"));
}
