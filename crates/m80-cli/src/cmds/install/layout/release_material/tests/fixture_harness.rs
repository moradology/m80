use std::fs;

use super::super::test_env::{fake_gh_fixture, valid_material, write_fake_curl, EnvVarGuard};
use super::super::test_fixture::{
    verifier_error_with_curl_log, write_direct_release_materials_with, ReleaseFixtureOptions,
};
use super::{assert_no_bundle_download, ReleaseMaterialPlan};

const MATERIAL_FIXTURE_CASES: &[(&str, &str, bool)] = &[
    ("bundle", "m80-linux-x86_64.tar.gz", true),
    ("bundle-metadata", "m80-linux-x86_64.bundle.json", false),
    ("asset-index", "m80-release-assets.json", false),
    ("install-script", "install.sh", false),
    ("bootstrap-selector", "m80-bootstrap-selector.tsv", false),
    ("release-build", "m80-release-build.json", false),
    (
        "release-integrity-predicate",
        "m80-release-integrity.json",
        false,
    ),
    (
        "release-attestation-bundle",
        "m80-release-integrity.attestation.jsonl",
        false,
    ),
    (
        "release-attestation-metadata",
        "m80-release-attestation.json",
        false,
    ),
];

#[test]
fn fixture_complete_material_records_requests_without_public_network() {
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
    let fixture =
        write_direct_release_materials_with(&material_dir, ReleaseFixtureOptions::default());

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );

    let verified = super::super::verify_official_release_bundle(&fixture.bundle_url)
        .unwrap()
        .unwrap();

    assert_eq!(verified.summary.install_sh_sha256, fixture.install_sha256);
    let log = fs::read_to_string(&log_path).unwrap();
    let requests = log.lines().collect::<Vec<_>>();
    assert!(
        requests
            .iter()
            .all(|url| url
                .starts_with("https://github.com/moradology/m80/releases/download/v0.0.0/")),
        "fake curl should record only fixture-backed official release URLs: {log}"
    );
    assert!(
        requests.iter().any(|url| *url == fixture.bundle_url),
        "complete fixture should reach bundle download after metadata verification: {log}"
    );
}

#[test]
fn fixture_omit_matrix_covers_every_direct_url_material_class() {
    let plan = ReleaseMaterialPlan::from_index_material(valid_material()).unwrap();
    let planned = plan
        .materials
        .iter()
        .map(|material| (material.class, material.name.as_str()))
        .collect::<Vec<_>>();
    let fixture_cases = MATERIAL_FIXTURE_CASES
        .iter()
        .map(|(class, name, _)| (*class, *name))
        .collect::<Vec<_>>();
    assert_eq!(
        planned, fixture_cases,
        "adding a direct URL verifier material class requires a fixture case"
    );

    for (class, name, expect_bundle_download) in MATERIAL_FIXTURE_CASES {
        let (err, log) = verifier_error_with_curl_log(ReleaseFixtureOptions {
            omit: Some(name),
            ..ReleaseFixtureOptions::default()
        });
        let message = err.to_string();
        if class.starts_with("asset-index") || *class == "release-integrity-predicate" {
            assert!(
                message.contains("release asset index"),
                "omitting {name} should fail in the asset-index fetcher: {message}"
            );
        } else {
            assert!(
                message.contains(&format!("material_class={class}")),
                "omitting {name} should fail with material class {class}: {message}"
            );
        }
        assert!(
            message.contains(name),
            "omitting {name} should name the missing fixture asset: {message}"
        );
        if !expect_bundle_download {
            assert_no_bundle_download(&log);
        }
    }
}

#[test]
fn fixture_negative_matrix_names_required_failure_shapes() {
    for scenario in [
        FixtureScenario {
            name: "mismatched tag",
            options: ReleaseFixtureOptions {
                wrong_release_tag: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release integrity predicate release_tag mismatch",
                "release asset index",
            ],
            expect_bundle_download: false,
        },
        FixtureScenario {
            name: "tampered bundle bytes",
            options: ReleaseFixtureOptions {
                tamper_bundle: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &["release integrity sha256 mismatch", "material_class=bundle"],
            expect_bundle_download: true,
        },
        FixtureScenario {
            name: "stale asset index",
            options: ReleaseFixtureOptions {
                stale_asset_index: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &["bundle digest mismatch", "material_class=bundle"],
            expect_bundle_download: true,
        },
        FixtureScenario {
            name: "stale bundle integrity subject",
            options: ReleaseFixtureOptions {
                wrong_bundle_checksum: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &["release integrity sha256 mismatch", "material_class=bundle"],
            expect_bundle_download: true,
        },
        FixtureScenario {
            name: "missing install.sh integrity subject",
            options: ReleaseFixtureOptions {
                missing_install_digest: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &["subject set mismatch", "install.sh"],
            expect_bundle_download: false,
        },
        FixtureScenario {
            name: "stale install.sh integrity subject",
            options: ReleaseFixtureOptions {
                stale_integrity_subjects: true,
                ..ReleaseFixtureOptions::default()
            },
            expected: &[
                "release integrity sha256 mismatch",
                "material_class=install-script",
            ],
            expect_bundle_download: false,
        },
        FixtureScenario {
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
        let (err, log) = verifier_error_with_curl_log(scenario.options);
        let message = err.to_string();
        for expected in scenario.expected {
            assert!(
                message.contains(expected),
                "{} missing {expected:?}: {message}",
                scenario.name
            );
        }
        if !scenario.expect_bundle_download {
            assert_no_bundle_download(&log);
        }
    }
}

#[derive(Clone, Copy)]
struct FixtureScenario {
    name: &'static str,
    options: ReleaseFixtureOptions,
    expected: &'static [&'static str],
    expect_bundle_download: bool,
}
