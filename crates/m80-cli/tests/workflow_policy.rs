use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should be under crates/")
        .to_path_buf()
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).expect("read repository file")
}

fn run_python_script(path: impl AsRef<Path>) {
    let path = path.as_ref();
    let output = Command::new("python3")
        .arg(path)
        .current_dir(repo_root())
        .output()
        .expect("run python script");
    assert!(
        output.status.success(),
        "{} failed\nstdout:\n{}\nstderr:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn workflow_policy_linter_accepts_repo_workflows() {
    run_python_script("scripts/lint-github-workflows.py");
}

#[test]
fn workflow_policy_linter_negative_fixture_suite_passes() {
    run_python_script("scripts/test-workflow-policy.py");
}

#[test]
fn actionlint_runner_fixture_suite_passes() {
    run_python_script("scripts/test-actionlint-runner.py");
}

#[test]
fn actionlint_live_fixture_suite_passes() {
    run_python_script("scripts/test-actionlint-fixtures.py");
}

#[test]
fn release_workflow_doc_records_authority_boundary() {
    let behavior = read_repo_file("docs/behaviors/ci/release-workflow-guardrails.md");
    let runbook = read_repo_file("docs/runbook/release-bundle.md");
    let release_workflow = read_repo_file(".github/workflows/release-artifacts.yml");

    for required in [
        "Top-level workflow permissions stay read-only",
        "The only release workflow job allowed to request `contents: write` is a\n  tag-gated publish job",
        "Release/latest workflows declare a concurrency group",
        "`scripts/lint-github-workflows.py`",
        "`scripts/test-workflow-policy.py`",
        "`scripts/run-actionlint.py`",
        "`scripts/test-actionlint-runner.py`",
        "`scripts/test-actionlint-fixtures.py`",
        "actionlint_1.7.12_linux_amd64.tar.gz",
        "Release artifact bytes cross from build to publish only through GitHub\n  workflow artifact identity",
        "workflow artifact id,\n  fixed artifact name, producing job id, source commit, release tag, and release\n  upload manifest digest",
        "manual\n  artifact override inputs, cache path injection, runner-local dist reuse, wrong\n  artifact id handoff",
        "Runner-local release scratch directories are not trust anchors",
        "fixed `/tmp` literals, preexisting scratch\n  path reuse, symlink staging roots, unsafe mode/owner checks, and clean\n  `$RUNNER_TEMP` usage",
        "invalid event syntax, duplicate job ids",
        "shellcheck failures\n  in inline bash run blocks",
        "documented non-bash exception",
        "concurrent CLI invocations\n  sharing one cache",
    ] {
        assert!(
            behavior.contains(required),
            "behavior doc missing required text: {required}"
        );
    }
    assert!(runbook.contains("`publish-release-artifacts`"));
    assert!(runbook.contains("`contents: write`"));
    assert!(runbook.contains("python3 scripts/run-actionlint.py --workflow-dir .github/workflows"));
    assert!(runbook.contains("Install `shellcheck` before running"));
    assert!(runbook.contains("python3 scripts/test-actionlint-fixtures.py"));
    assert!(runbook.contains("8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"));
    assert!(read_repo_file(".github/workflows/ci.yml").contains("workflow syntax lint"));
    assert!(read_repo_file(".github/workflows/ci.yml").contains("shellcheck --version"));
    assert!(release_workflow.contains("publish-release-artifacts:"));
    assert!(release_workflow.contains("contents: write"));
    assert!(release_workflow.contains("release-artifacts-${{ github.ref_name }}"));
    assert!(release_workflow.contains("release_dist_artifact_id"));
    assert!(release_workflow.contains("release_dist_producer_job"));
    assert!(release_workflow
        .contains("$RUNNER_TEMP/m80-release-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-build"));
    assert!(release_workflow.contains("test ! -e \"$release_tmp_root\""));
    assert!(release_workflow.contains("test ! -L \"$release_tmp_root\""));
    assert!(release_workflow.contains(
        "artifact-ids: ${{ needs.build-release-artifacts.outputs.release_dist_artifact_id }}"
    ));
    assert!(release_workflow.contains("merge-multiple: true"));
    assert!(release_workflow.contains("Verify workflow artifact origin handoff"));
}
