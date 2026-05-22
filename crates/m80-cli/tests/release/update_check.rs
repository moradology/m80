use std::fs;
use std::path::PathBuf;

#[test]
fn update_check_doc_names_read_only_states_and_outputs() {
    let doc = read_repo_file("docs/behaviors/release/update-check.md");
    let readme = read_repo_file("README.md");
    let crate_readme = read_repo_file("crates/m80-cli/README.md");
    let runbook = read_repo_file("docs/runbook/release.md");

    for required in [
        "`m80-o3uh9.16.18.1`",
        "`m80-o3uh9.16.9.2`",
        "`m80 update --check`",
        "`current`",
        "`outdated`",
        "`yanked`",
        "`unsafe`",
        "`unknown_offline`",
        "`stale_latest_metadata`",
        "`local_dev_install`",
        "`install_unhealthy`",
        "`active_tag`",
        "`latest_stable_tag`",
        "`safety_floor.status`",
        "`proof_cache_age_seconds`",
        "`latest_status_origin`",
        "`latest_status_cache_state`",
        "`latest_status_fetched_at`",
        "`latest_status_max_age_seconds`",
        "`latest_status_offline_reason`",
        "`apply_command`",
        "`reinstall_command`",
        "`retry_command`",
        "no apply command is emitted when the latest target is yanked or below\nthe safety floor",
        "never flips `<install-root>/active`",
        "never rewrites profiles",
        "never downloads a bundle",
        "never refreshes the proof cache or latest-status cache",
        "Malformed freshness metadata fails closed",
        "`m80-latest-freshness-proof.json`",
        "`--latest-status <path>`",
        "`--latest-status-url <url>`",
        "`latest_status_cache_state: \"stale\"`",
    ] {
        assert!(
            doc.contains(required),
            "update-check doc missing {required:?}"
        );
    }

    for required in [
        "m80 update --check",
        "without touching the\ninstall root",
        "`stale_latest_metadata`",
        "`prerelease_active`",
        "`ineligible_active`",
        "`local_dev_install`",
        "`install_unhealthy`",
        "exact pinned install command",
        "fallback cache",
        "retry command",
    ] {
        assert!(readme.contains(required), "README missing {required:?}");
    }

    for required in [
        "`m80 update --check [--install-root <path>] [--profile <name>]`",
        "without writing the install root",
        "`stale_latest_metadata`",
        "`prerelease_active`",
        "`ineligible_active`",
        "`local_dev_install`",
        "`install_unhealthy`",
        "`docs/behaviors/release/update-check.md`",
        "read-only fallback cache",
        "offline reason",
    ] {
        assert!(
            crate_readme.contains(required),
            "crate README missing {required:?}"
        );
    }

    for required in [
        "m80 update --check",
        "does not write the install root",
        "`stale_latest_metadata`",
        "`prerelease_active`",
        "`ineligible_active`",
        "`local_dev_install`",
        "`install_unhealthy`",
        "docs/behaviors/release/update-check.md",
    ] {
        assert!(runbook.contains(required), "runbook missing {required:?}");
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
