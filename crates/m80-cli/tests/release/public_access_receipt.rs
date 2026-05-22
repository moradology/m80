use std::fs;
use std::path::PathBuf;

#[test]
fn public_access_receipt_doc_workflow_and_tests_pin_no_auth_latest_gate() {
    let doc = read_repo_file("docs/behaviors/release/public-access-receipt.md");
    let runbook = read_repo_file("docs/runbook/release.md");
    let workflow = read_repo_file(".github/workflows/release-artifacts.yml");
    let ci = read_repo_file(".github/workflows/ci.yml");

    for required in [
        "`release-readiness-public-access.json`",
        "`public-access-latest`",
        "`public-access-proof`",
        "`GH_TOKEN`",
        "`GITHUB_TOKEN`",
        "`gh_auth_present`",
        "`/releases/latest/download/install.sh`",
        "Fixture receipts are useful",
    ] {
        assert!(
            doc.contains(required),
            "public-access doc missing {required:?}"
        );
    }

    assert!(
        runbook.contains("release-readiness-public-access.json")
            && runbook.contains("fixture receipts cannot satisfy")
            && runbook.contains("before latest promotion"),
        "release runbook must require the no-auth public-access receipt before latest promotion"
    );
    assert!(
        workflow.contains("Write no-auth public-access release readiness receipt")
            && workflow.contains("scripts/release_public_access_receipt.py")
            && workflow.contains("GH_CONFIG_DIR=/tmp/m80-noauth-gh")
            && workflow.contains("env -u GH_TOKEN -u GITHUB_TOKEN")
            && workflow.contains("m80-release-public-access-${{ github.run_id }}"),
        "release workflow must generate and upload the no-auth public-access receipt"
    );
    assert!(
        workflow
            .find("Write no-auth public-access release readiness receipt")
            .expect("public-access receipt step missing")
            < workflow
                .find("Mark validated release as latest")
                .expect("latest promotion step missing"),
        "public-access receipt must be generated before latest promotion"
    );
    assert!(
        ci.contains("scripts/release_public_access_receipt.py")
            && ci.contains("scripts/test-release-public-access-receipt.py"),
        "CI must compile and test the public-access receipt verifier"
    );
}

fn read_repo_file(path: &str) -> String {
    fs::read_to_string(repo_root().join(path)).unwrap_or_else(|error| {
        panic!("failed to read repo file {path}: {error}");
    })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate lives under repo/crates/m80-cli")
        .to_path_buf()
}
