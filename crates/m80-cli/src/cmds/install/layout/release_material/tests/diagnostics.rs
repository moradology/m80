use m80_firecracker::FcError;

use super::super::test_fixture::{verifier_error, ReleaseFixtureOptions};

#[test]
fn failure_context_names_release_tag_material_class_and_one_retry() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    let err = verifier_error(ReleaseFixtureOptions {
        wrong_release_tag: true,
        ..ReleaseFixtureOptions::default()
    });

    let err = super::super::with_install_retry_context(err, &bundle_url, &install_root);
    let message = err.to_string();

    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(
        message.contains("material_class=release-integrity-predicate"),
        "{message}"
    );
    assert_eq!(message.matches("retry_command=").count(), 1, "{message}");
    assert!(
        message.contains("m80 install --bundle-url 'https://github.com/moradology/m80/releases/download/v0.0.0/m80-linux-x86_64.tar.gz'"),
        "{message}"
    );
    assert!(message.contains("--install-root"), "{message}");
}

#[test]
fn classifier_failure_context_names_classifier_class_and_one_retry() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_url = "https://github.com/moradology/m80/releases/latest/download/install.sh";
    let err = FcError::UnsupportedOperation {
        operation: "m80 install",
        reason:
            "bundle URL must use file://, https:// release assets, or local http:// test fixtures"
                .into(),
    };

    let err = super::super::with_install_retry_context(err, bundle_url, &install_root);
    let message = err.to_string();

    assert!(
        message.contains("material_class=direct-url-classifier"),
        "{message}"
    );
    assert_eq!(message.matches("retry_command=").count(), 1, "{message}");
    assert!(
        message.contains(
            "m80 install --bundle-url 'https://github.com/moradology/m80/releases/latest/download/install.sh'"
        ),
        "{message}"
    );
    assert!(message.contains("--install-root"), "{message}");
}

#[test]
fn retry_context_makes_relative_install_root_absolute() {
    let install_root = std::path::Path::new("relative-install-root");
    let bundle_url = crate::release_urls::release_asset_url("v0.0.0", "m80-linux-x86_64.tar.gz");
    let err = verifier_error(ReleaseFixtureOptions {
        wrong_release_tag: true,
        ..ReleaseFixtureOptions::default()
    });

    let err = super::super::with_install_retry_context(err, &bundle_url, install_root);
    let message = err.to_string();
    let expected_root = std::env::current_dir()
        .unwrap()
        .join(install_root)
        .display()
        .to_string();

    assert!(
        message.contains(&format!("--install-root '{expected_root}'")),
        "{message}"
    );
    assert!(
        !message.contains("--install-root 'relative-install-root'"),
        "{message}"
    );
}

#[test]
fn retry_context_shell_quotes_url_and_install_root() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("root's");
    let bundle_url = "https://example.invalid/releases/download/v0.0.0/m80's.tar.gz";
    let err = FcError::UnsupportedOperation {
        operation: "m80 install",
        reason:
            "bundle URL must use file://, https:// release assets, or local http:// test fixtures"
                .into(),
    };

    let err = super::super::with_install_retry_context(err, bundle_url, &install_root);
    let message = err.to_string();
    let expected = format!(
        "retry_command=m80 install --bundle-url {} --install-root {}",
        shell_quote(bundle_url),
        shell_quote(&install_root.display().to_string())
    );

    assert!(message.contains(&expected), "{message}");
    assert_eq!(message.matches("retry_command=").count(), 1, "{message}");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
