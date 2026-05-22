use std::fs;
use std::path::PathBuf;

#[test]
fn update_check_doc_names_read_only_states_and_outputs() {
    let doc = read_repo_file("docs/behaviors/release/update-check.md");
    let readme = read_repo_file("README.md");
    let crate_readme = read_repo_file("crates/m80-cli/README.md");
    let runbook = read_repo_file("docs/runbook/release.md");
    let install_state = read_repo_file("docs/behaviors/release/install-state.md");

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
        "`active_kind`",
        "`freshness_state`",
        "`latest_stable_tag`",
        "`safety_state`",
        "`safety_floor.status`",
        "`safety_floor.replacement_command`",
        "`safety_floor.metadata_source`",
        "`proof_cache_age_seconds`",
        "`latest_status_origin`",
        "`latest_status_cache_state`",
        "`latest_status_fetched_at`",
        "`latest_status_max_age_seconds`",
        "`latest_status_offline_reason`",
        "`apply_command`",
        "`reinstall_command`",
        "`retry_command`",
        "unsafe or yanked state uses the safety policy's pinned `replacement_command`",
        "advisory release metadata unless a separate release policy or CI gate names\nthe status artifact as release-blocking",
        "There is no hidden `release_blocking`\nfield in this schema",
        "it does not stop `m80 run` and it never updates",
        "[`freshness-failure-policy.md`](freshness-failure-policy.md)",
        "safety_floor_replacement_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh",
        "never flips `<install-root>/active`",
        "never rewrites profiles",
        "never downloads a bundle",
        "never refreshes the proof cache or latest-status cache",
        "Malformed freshness metadata fails closed",
        "`m80-latest-freshness-proof.json`",
        "`--latest-status <path>`",
        "`--latest-status-url <url>`",
        "`latest_status_cache_state: \"stale\"`",
        "`stale_active_metadata`",
    ] {
        assert!(
            doc.contains(required),
            "update-check doc missing {required:?}"
        );
    }

    for required in [
        "m80 update --check",
        "If it prints a `next_command=...`, run that command.",
    ] {
        assert!(readme.contains(required), "README missing {required:?}");
    }

    for required in [
        "`m80 update --check [--install-root <path>] [--profile <name>]`",
        "without writing the install root",
        "The safety floor is advisory unless an explicit\n  release policy or CI gate names the status artifact as release-blocking input",
        "`--check` never updates the install by itself",
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
        "`m80-latest-freshness-proof.json` from the public latest GitHub release asset\nset",
        "fetches only that status asset by default",
        "Safety-floor data is advisory unless a separate release policy\nor CI gate names this status artifact as release-blocking input",
        "`safety_floor` object has no blocking boolean",
        "[`freshness-failure-policy.md`](../behaviors/release/freshness-failure-policy.md)",
        "previous public status remains the last trusted status",
        "update the explicit policy\nor CI gate in the same change",
    ] {
        assert!(runbook.contains(required), "runbook missing {required:?}");
    }

    for required in [
        "`m80 update --check` reuses this installed-state reader",
        "`latest_status_source`",
        "`latest_status_origin`",
        "`latest_status_cache_state`",
        "`latest_status_fetched_at`",
        "`latest_status_max_age_seconds`",
        "`latest_status_offline_reason`",
        "`safety_state`",
        "`safety_floor.status`",
        "`proof_cache_status`",
        "`proof_cache_age_seconds`",
        "`active_kind` is one of\n`stable_release`, `prerelease`, `ineligible`, `local_dev`, `missing_active`, or\n`stale_active_metadata`",
        "`freshness_state` is one of `current`, `outdated`,\n`unknown_offline`, `stale_latest_metadata`, `prerelease_active`,\n`ineligible_active`, `local_dev_install`, `install_unhealthy`, `yanked`, or\n`unsafe`",
        "[`update-check.md`](update-check.md)",
    ] {
        assert!(
            install_state.contains(required),
            "install-state doc missing {required:?}"
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
