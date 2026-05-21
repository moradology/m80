use std::fs;
use std::path::PathBuf;

#[test]
fn freshness_failure_policy_doc_names_taxonomy_and_ci_contract() {
    let doc = read_repo_file("docs/behaviors/release/freshness-failure-policy.md");
    let runbook = read_repo_file("docs/runbook/release.md");

    for required in [
        "`m80-o3uh9.21.2.1`",
        "`docs/behaviors/release/freshness-failure-policy.json`",
        "python3 scripts/verify-freshness-failure-policy.py",
        "`network-transient`",
        "`stale-latest`",
        "`missing-public-asset`",
        "`docs-drift`",
        "`checksum-mismatch`",
        "`provenance-mismatch`",
        "`real-kvm-substrate-unavailable`",
        "`verifier-schema-drift`",
        "`retry-only`",
        "`open-update-bead`",
        "`block-next-release-latest`",
        "`page-maintainer`",
        "`manual-operator-confirmation`",
        "verifier-emitted class absent from the config",
    ] {
        assert!(
            doc.contains(required),
            "freshness failure policy doc missing {required:?}"
        );
    }

    for required in [
        "`docs/behaviors/release/freshness-failure-policy.json`",
        "`python3 scripts/verify-freshness-failure-policy.py`",
        "retry-only, open/update one repair bead, block the next",
        "CI rejects verifier-emitted classes",
    ] {
        assert!(
            runbook.contains(required),
            "release runbook missing freshness failure policy text {required:?}"
        );
    }
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).expect("read repository file")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}
