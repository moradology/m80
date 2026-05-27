use super::super::super::test_fixture::ReleaseFixtureOptions;

#[test]
fn missing_integrity_predicate_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing integrity predicate",
        options: ReleaseFixtureOptions {
            omit: Some("m80-release-integrity.json"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release asset index fetch failed",
            "release_tag=v0.0.0",
            "material_class=asset-index",
            "m80-release-integrity.json",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn missing_attestation_bundle_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing attestation bundle",
        options: ReleaseFixtureOptions {
            omit: Some("m80-release-integrity.attestation.jsonl"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material fetch failed",
            "release_tag=v0.0.0",
            "material_class=release-attestation-bundle",
            "m80-release-integrity.attestation.jsonl",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn missing_attestation_metadata_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing attestation metadata",
        options: ReleaseFixtureOptions {
            omit: Some("m80-release-attestation.json"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material fetch failed",
            "release_tag=v0.0.0",
            "material_class=release-attestation-metadata",
            "m80-release-attestation.json",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn missing_asset_index_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing asset index",
        options: ReleaseFixtureOptions {
            omit: Some("m80-release-assets.json"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material plan failed",
            "release_tag=v0.0.0",
            "release asset index",
            "m80-release-assets.json",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn missing_bundle_metadata_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing bundle metadata",
        options: ReleaseFixtureOptions {
            omit: Some("m80-linux-x86_64.bundle.json"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material fetch failed",
            "release_tag=v0.0.0",
            "material_class=bundle-metadata",
            "m80-linux-x86_64.bundle.json",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn missing_bootstrap_selector_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing bootstrap selector",
        options: ReleaseFixtureOptions {
            omit: Some("m80-bootstrap-selector.tsv"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material fetch failed",
            "release_tag=v0.0.0",
            "material_class=bootstrap-selector",
            "m80-bootstrap-selector.tsv",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn trust_policy_signer_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "trust-policy signer mismatch",
        options: ReleaseFixtureOptions {
            wrong_attestation_signer: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "trust-policy",
            "signer_identity",
            "mismatch",
            "release_tag=v0.0.0",
            "material_class=release-attestation-metadata",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn integrity_subject_digest_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "integrity subject digest mismatch",
        options: ReleaseFixtureOptions {
            stale_integrity_subjects: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release integrity sha256 mismatch",
            "release_tag=v0.0.0",
            "material_class=install-script",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn integrity_predicate_digest_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "integrity predicate digest mismatch",
        options: ReleaseFixtureOptions {
            bad_predicate_subject: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release integrity sha256 mismatch",
            "release_tag=v0.0.0",
            "material_class=install-script",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn asset_index_digest_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "asset index digest mismatch",
        options: ReleaseFixtureOptions {
            stale_asset_index: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material bundle digest mismatch",
            "release_tag=v0.0.0",
            "material_class=bundle",
        ],
        expect_bundle_download: true,
    });
}

#[test]
fn indexed_bundle_size_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "indexed bundle size mismatch",
        options: ReleaseFixtureOptions {
            stale_bundle_size: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material bundle size mismatch",
            "expected_size_bytes=",
            "observed_size_bytes=",
            "release_tag=v0.0.0",
            "material_class=bundle",
        ],
        expect_bundle_download: true,
    });
}

#[test]
fn native_attestation_bundle_failure_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "bad attestation",
        options: ReleaseFixtureOptions {
            gh_failure: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release attestation bundle mediaType mismatch",
            "release_tag=v0.0.0",
            "material_class=release-attestation-bundle",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn repository_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "wrong repo",
        options: ReleaseFixtureOptions {
            wrong_repository: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release integrity repository mismatch",
            "release_tag=v0.0.0",
            "material_class=release-integrity-predicate",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn release_tag_mismatch_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "wrong tag",
        options: ReleaseFixtureOptions {
            wrong_release_tag: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release integrity predicate release_tag mismatch",
            "release_tag=v0.0.0",
            "material_class=asset-index",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn missing_install_script_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "missing install script",
        options: ReleaseFixtureOptions {
            omit: Some("install.sh"),
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release material fetch failed",
            "release_tag=v0.0.0",
            "material_class=install-script",
        ],
        expect_bundle_download: false,
    });
}

#[test]
fn tampered_bundle_aborts_before_install_root_mutation() {
    assert_failure_preserves_install_root(FailureScenario {
        name: "tampered bundle",
        options: ReleaseFixtureOptions {
            tamper_bundle: true,
            ..ReleaseFixtureOptions::default()
        },
        expected: &[
            "release integrity sha256 mismatch",
            "release_tag=v0.0.0",
            "material_class=bundle",
        ],
        expect_bundle_download: true,
    });
}

#[derive(Clone, Copy)]
struct FailureScenario {
    name: &'static str,
    options: ReleaseFixtureOptions,
    expected: &'static [&'static str],
    expect_bundle_download: bool,
}

fn assert_failure_preserves_install_root(scenario: FailureScenario) {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let before = super::seed_install_root_snapshot(&install_root);

    let (err, log) = super::install_layout_error_with_curl_log(&install_root, scenario.options);

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
        super::snapshot_install_root(&install_root),
        before,
        "{} mutated install root before release proof material verification completed",
        scenario.name
    );
    if !scenario.expect_bundle_download {
        super::super::assert_no_bundle_download(&log);
    }
}
