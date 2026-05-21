use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use m80_firecracker::FcError;

use super::super::test_env::{
    fake_gh_fixture, official_release_plan, write_fake_curl, EnvVarGuard,
};
use super::super::test_fixture::{write_direct_release_materials_with, ReleaseFixtureOptions};
use super::assert_no_bundle_download;

#[test]
fn official_release_verifier_failure_matrix_leaves_install_root_unchanged() {
    for scenario in [
        FailureScenario {
            name: "missing metadata",
            options: ReleaseFixtureOptions {
                omit: Some("m80-release-attestation.json"),
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release material fetch failed",
                "material_class=release-attestation-metadata",
            ],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "wrong repo",
            options: ReleaseFixtureOptions {
                wrong_repository: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release integrity repository mismatch",
                "material_class=release-integrity-predicate",
            ],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "wrong tag",
            options: ReleaseFixtureOptions {
                wrong_release_tag: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release integrity release_tag mismatch",
                "material_class=release-integrity-predicate",
            ],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "tampered bundle",
            options: ReleaseFixtureOptions {
                tamper_bundle: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &["sidecar digest mismatch", "material_class=bundle"],
            expect_bundle_download: true,
        },
        FailureScenario {
            name: "stale SHA256SUMS",
            options: ReleaseFixtureOptions {
                stale_public_sha256s: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "public SHA256SUMS mismatch",
                "material_class=install-script",
            ],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "stale asset index",
            options: ReleaseFixtureOptions {
                stale_asset_index: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &["checksum mismatch", "material_class=bundle-checksum"],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "bad attestation",
            options: ReleaseFixtureOptions {
                gh_failure: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "cryptographic attestation verification failed",
                "material_class=release-attestation-bundle",
            ],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "bad predicate",
            options: ReleaseFixtureOptions {
                bad_predicate_subject: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release integrity sha256 mismatch",
                "material_class=install-script",
            ],
            expect_bundle_download: false,
        },
        FailureScenario {
            name: "network failure",
            options: ReleaseFixtureOptions {
                omit: Some("install.sh"),
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release material fetch failed",
                "material_class=install-script",
            ],
            expect_bundle_download: false,
        },
    ] {
        let temp = tempfile::tempdir().unwrap();
        let install_root = temp.path().join("install-root");
        let before = seed_install_root_snapshot(&install_root);

        let (err, log) = install_layout_error_with_curl_log(&install_root, scenario.options);

        let message = err.to_string();
        for expected in scenario.expected {
            assert!(
                message.contains(expected),
                "{} missing {expected:?}: {message}",
                scenario.name
            );
        }
        assert!(
            message.contains("retry_command=m80 install --bundle-url"),
            "{} missing retry command: {message}",
            scenario.name
        );
        assert!(
            message.contains("--install-root"),
            "{} missing install root in retry command: {message}",
            scenario.name
        );
        assert_eq!(
            snapshot_install_root(&install_root),
            before,
            "{} mutated install root before verification completed",
            scenario.name
        );
        if !scenario.expect_bundle_download {
            assert_no_bundle_download(&log);
        }
    }
}

#[derive(Clone, Copy)]
struct FailureScenario {
    name: &'static str,
    options: ReleaseFixtureOptions,
    expected: &'static [&'static str],
    expect_bundle_download: bool,
}

fn install_layout_error_with_curl_log(
    install_root: &Path,
    options: ReleaseFixtureOptions,
) -> (FcError, String) {
    let _guard = super::super::super::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture = write_direct_release_materials_with(&material_dir, options);

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        "0123456789abcdef0123456789abcdef01234567",
    );
    let _gh_failure = options
        .gh_failure
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_FAIL", "1"));
    let _gh_omit_subject = options
        .gh_omit_subject
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_OMIT_SUBJECT", "1"));
    let _gh_wrong_subject_digest = options
        .gh_wrong_subject_digest
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_WRONG_SUBJECT_DIGEST", "1"));
    let _gh_wrong_source_ref = options
        .gh_wrong_source_ref
        .then(|| EnvVarGuard::set_value("M80_FAKE_GH_EXPECT_SOURCE_REF", "refs/tags/v9.9.9"));

    let err = match super::super::super::install_bundle_layout(&official_release_plan(install_root))
    {
        Ok(_) => {
            panic!(
                "scenario unexpectedly installed verified bundle: {}",
                fixture.bundle_url
            )
        }
        Err(err) => err,
    };
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    (err, log)
}

#[derive(Debug, PartialEq, Eq)]
enum InstallSnapshotEntry {
    Dir,
    File(Vec<u8>),
    Symlink(PathBuf),
}

fn seed_install_root_snapshot(install_root: &Path) -> BTreeMap<PathBuf, InstallSnapshotEntry> {
    let previous = install_root.join("versions/v-previous");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("marker"), b"previous install\n").unwrap();
    fs::create_dir_all(install_root.join("profiles")).unwrap();
    fs::write(
        install_root.join("profiles/default.toml"),
        b"description = \"old profile\"\n",
    )
    .unwrap();
    fs::write(
        install_root.join("config.toml"),
        b"default_profile = 'old'\n",
    )
    .unwrap();
    let stale = install_root.join(".staging/layout-stale");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("partial"), b"stale partial\n").unwrap();
    symlink(&previous, install_root.join("active")).unwrap();
    snapshot_install_root(install_root)
}

fn snapshot_install_root(root: &Path) -> BTreeMap<PathBuf, InstallSnapshotEntry> {
    let mut entries = BTreeMap::new();
    if root.exists() {
        capture_snapshot(root, Path::new(""), &mut entries);
    }
    entries
}

fn capture_snapshot(
    path: &Path,
    relative: &Path,
    entries: &mut BTreeMap<PathBuf, InstallSnapshotEntry>,
) {
    let metadata = fs::symlink_metadata(path).unwrap();
    if metadata.file_type().is_symlink() {
        entries.insert(
            relative.to_path_buf(),
            InstallSnapshotEntry::Symlink(fs::read_link(path).unwrap()),
        );
    } else if metadata.is_dir() {
        if !relative.as_os_str().is_empty() {
            entries.insert(relative.to_path_buf(), InstallSnapshotEntry::Dir);
        }
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            capture_snapshot(&entry.path(), &relative.join(entry.file_name()), entries);
        }
    } else {
        entries.insert(
            relative.to_path_buf(),
            InstallSnapshotEntry::File(fs::read(path).unwrap()),
        );
    }
}
