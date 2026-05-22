use std::fs;
use std::path::PathBuf;

#[test]
fn install_state_doc_names_resolver_states_and_guards() {
    let doc = read_repo_file("docs/behaviors/release/install-state.md");

    for required in [
        "`m80-o3uh9.16.7.2`",
        "`m80-o3uh9.16.7.1`",
        "`m80 install-status`",
        "`m80 --json install-status`",
        "`healthy_active_release`",
        "`missing_active_pointer`",
        "`dangling_active_pointer`",
        "`local_dev_tree`",
        "`stale_profile_target`",
        "`explicit_override`",
        "`missing_install_metadata`",
        "`stale_install_metadata`",
        "`tampered_proof_cache`",
        "`invalid_install_metadata`",
        "`m80-o3uh9.16.7.3`",
        "`m80-o3uh9.16.7.4`",
        "`m80-o3uh9.16.7.5`",
        "`m80-o3uh9.16.7.6`",
        "`m80-o3uh9.16.8.4`",
        "bundle.json",
        "install-provenance.json",
        "host-binaries.manifest.json",
        "release-proof-cache/manifest.json",
        "`schema_version: 1`",
        "`next_action`",
        "`mismatches`",
        "JSON field table",
        "| `schema_version` |",
        "| `active.release_tag` |",
        "`selected_config.explicit_override`",
        "`metadata.proof_cache_manifest.path/status/sha256`",
        "`proof_cache.status`",
        "`proof_cache.cache_dir`",
        "`proof_cache.manifest_sha256`",
        "`proof_cache.manifest_digest`",
        "`proof_cache.materials[]`",
        "`proof_cache.trust_policy.path/identity/sha256`",
        "`proof_cache.verifier_versions.*`",
        "`proof_cache.diagnostics[]`",
        "`proof_cache.repair_command`",
        "`missing_active_install`",
        "`local_dev_install`",
        "evidence of what the installer verified at install time",
        "never fetches release metadata",
        "mode-changed",
        "blocks reuse",
        "Repair examples:",
        "status=missing_active_pointer",
        "status=explicit_override",
        "`expected_tag`",
        "`observed_tag`",
        "`explicit_profile_override`",
        "`install_root_override`",
        "`status_matrix_tampered_proof_cache`",
        "`resolver_reports_tampered_proof_cache_for_manifest_digest_mismatch`",
        "`resolver_reports_tampered_proof_cache_for_changed_file_mode`",
        "`resolver_reports_tampered_proof_cache_for_changed_manifest_mode`",
        "`resolver_reports_tampered_proof_cache_for_changed_cache_dir_mode`",
        "`preflight_json_report_reads_offline_proof_cache_material`",
        "`preflight_json_report_reports_tampered_proof_cache_repair_command`",
        "`deny_unknown_fields`",
        "`O_NOFOLLOW`",
        "symlinked installed references",
        "does not execute",
        "installed binaries",
        "rejects active-pointer traversal",
        "install-owned profile",
        "paths outside the install root",
        "`M80_DEFAULT_PROFILE`",
        "`--profile`",
    ] {
        assert!(
            doc.contains(required),
            "install-state doc missing {required:?}"
        );
    }
}

#[test]
fn install_state_docs_keep_quickstart_and_release_evidence_repairable() {
    let readme = read_repo_file("README.md");
    let runbook = read_repo_file("docs/runbook/release.md");
    let install_state = read_repo_file("docs/behaviors/release/install-state.md");

    for required in [
        "If something fails, start with:",
        "m80 install-status",
        "Then check the host:",
        "m80 preflight",
        "github.com/moradology/m80/releases/latest/download/install.sh",
    ] {
        assert!(readme.contains(required), "README missing {required:?}");
    }

    for required in [
        "## Installed Status Evidence",
        "m80 --json install-status > install-status.json",
        "`active.release_tag` and `active.install_dir`",
        "`selected_config.explicit_override`",
        "`metadata.proof_cache_manifest.path`",
        "`proof_cache.status`",
        "`proof_cache.manifest_digest`",
        "`proof_cache.repair_command`",
        "`diagnostics` and `mismatches`: must be empty",
        "`next_action.kind`: must be `ready`",
        "does not replace the real-KVM smoke",
    ] {
        assert!(
            runbook.contains(required),
            "release runbook missing {required:?}"
        );
    }

    for (name, doc) in [
        ("README.md", readme.as_str()),
        ("docs/runbook/release.md", runbook.as_str()),
        (
            "docs/behaviors/release/install-state.md",
            install_state.as_str(),
        ),
    ] {
        assert!(
            !doc.contains("raw.githubusercontent.com/moradology/m80"),
            "{name} should not use raw main installer URLs"
        );
        assert!(
            !doc.contains("/releases/latest/download/m80-"),
            "{name} should not use direct artifact-only latest bundle selectors"
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
