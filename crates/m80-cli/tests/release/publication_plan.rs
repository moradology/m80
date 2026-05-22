use std::fs;
use std::path::PathBuf;

#[test]
fn publication_plan_doc_and_workflow_keep_release_publish_fail_closed() {
    let doc = read_repo_file("docs/behaviors/release/publication-plan.md");
    let workflow = read_repo_file(".github/workflows/release-artifacts.yml");
    let runbook = read_repo_file("docs/runbook/release.md");

    for required in [
        "`m80-release-publication-plan.json`",
        "`create_draft_upload_publish`",
        "`validate_existing_public_release`",
        "`fail_manual_recovery_required`",
        "public assets without `--clobber`",
        "pre-promotion",
        "same-size byte drift fails during the remote asset inventory",
        "gh release delete <version> --yes",
        "scripts/test-release-publication-plan.py",
    ] {
        assert!(
            doc.contains(required),
            "publication-plan doc missing {required:?}"
        );
    }

    assert!(
        workflow.contains("Resolve release publication plan")
            && workflow.contains("scripts/release_publication_plan.py")
            && workflow.contains("/tmp/m80-release-upload/m80-release-publication-plan.json"),
        "release workflow must write the publication plan before release mutation"
    );
    assert!(
        workflow.contains("gh release create \"$GITHUB_REF_NAME\"")
            && workflow.contains("--draft")
            && workflow.contains("--verify-tag")
            && workflow.contains("gh release upload \"$GITHUB_REF_NAME\" \"${upload_paths[@]}\"")
            && workflow.contains("Validate uploaded draft before latest promotion")
            && workflow.contains(
                "gh release edit \"$GITHUB_REF_NAME\" --draft=false --latest --verify-tag"
            ),
        "release workflow must create a draft, upload immutable assets, validate them, then publish latest"
    );
    assert!(
        !workflow.contains("--clobber"),
        "release workflow must not clobber existing public assets"
    );
    assert!(
        workflow.contains("validate_existing_public_release)")
            && workflow.contains("validating remote bytes without upload"),
        "release workflow must support validate-only reruns over existing public releases"
    );
    assert!(
        runbook.contains("m80-release-publication-plan.json")
            && runbook.contains("after that pre-promotion")
            && runbook.contains("skips upload and validates the served bytes")
            && runbook.contains("delete only the"),
        "release runbook must document publication-plan rerun recovery"
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
