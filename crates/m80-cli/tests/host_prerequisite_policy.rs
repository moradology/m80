use std::fs;
use std::path::PathBuf;

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

fn readme_preflight_paragraph_bounds(readme: &str) -> (usize, usize) {
    let start = readme
        .find("`m80 preflight` reports missing host setup before launch")
        .expect("README should describe preflight remediation");
    let end = readme[start..]
        .find("\n\nProduction operators")
        .map(|offset| start + offset)
        .expect("README preflight paragraph should end before production operator docs");
    (start, end)
}

fn readme_preflight_paragraph(readme: &str) -> &str {
    let (start, end) = readme_preflight_paragraph_bounds(readme);
    &readme[start..end]
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(haystack.contains(needle), "missing required text: {needle}");
}

fn assert_contains_words(haystack: &str, needle: &str) {
    let normalized_haystack = haystack.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized_needle = needle.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalized_haystack.contains(&normalized_needle),
        "missing required text: {needle}"
    );
}

#[test]
fn policy_doc_declares_v0x_operator_provided_prerequisites() {
    let policy = read_repo_file("docs/behaviors/release/host-prerequisite-policy.md");

    for required in [
        "m80 v0.x treats Firecracker, the official jailer, and Firecracker's seccomp\nfilter as operator-provided host prerequisites",
        "It does not bundle or install the official Firecracker\nVMM, the official jailer, or the Firecracker seccomp filter.",
        "Bundled Firecracker mode is unsupported for v0.x.",
        "`M80_FIRECRACKER_BIN`",
        "`M80_JAILER_BIN`",
        "`M80_FIRECRACKER_SECCOMP_FILTER`",
        "`/dev/kvm` present and writable",
        "cgroup v2 available when `M80_CGROUP_MODE=unified-v2`",
        "startup privilege through root",
    ] {
        assert_contains(&policy, required);
    }
}

#[test]
fn policy_doc_names_version_and_cve_sources_of_truth() {
    let policy = read_repo_file("docs/behaviors/release/host-prerequisite-policy.md");

    for required in [
        "`expected_firecracker_version`",
        "`bundle.json`",
        "`M80_FIRECRACKER_VERSION`",
        "jailer version source of truth is the accepted Firecracker train",
        "same Firecracker release train",
        "`crates/m80-preflight/src/firecracker_train.rs`",
        "`FirecrackerTrainPolicy::from_expected_firecracker_version`",
        "`host-binaries.manifest.json`",
        "`crates/m80-preflight/src/cve_floor.rs`",
        "`docs/security/firecracker-cve-floor.md`",
    ] {
        assert_contains(&policy, required);
    }
}

#[test]
fn policy_doc_requires_preflight_proof_rows_for_release_artifacts() {
    let policy = read_repo_file("docs/behaviors/release/host-prerequisite-policy.md");

    assert_contains(&policy, "`m80 preflight --json`");
    assert_contains_words(
        &policy,
        "`Firecracker binary` row records the expected and observed Firecracker version",
    );
    assert_contains_words(
        &policy,
        "`Jailer binary` row records the expected jailer version and the observed jailer version",
    );
    assert_contains_words(
        &policy,
        "operator-provided train matches the guest artifact set",
    );
}

#[test]
fn readme_links_policy_only_from_preflight_remediation_context() {
    let readme = read_repo_file("README.md");
    let link = "docs/behaviors/release/host-prerequisite-policy.md";

    let link_count = readme.match_indices(link).count();
    assert_eq!(
        link_count, 1,
        "README should link the policy once from remediation context"
    );

    let (preflight_index, _) = readme_preflight_paragraph_bounds(&readme);
    let link_index = readme.find(link).expect("README should link policy");
    let diagnostics_index = readme
        .find("## Diagnostics")
        .expect("README should have diagnostics section");

    assert!(
        preflight_index < link_index && link_index < diagnostics_index,
        "policy link must stay in the quickstart preflight/remediation paragraph"
    );
}

#[test]
fn readme_preflight_paragraph_keeps_common_host_prerequisite_story() {
    let readme = read_repo_file("README.md");
    let paragraph = readme_preflight_paragraph(&readme);

    for required in [
        "Linux/KVM host",
        "`/dev/kvm` access",
        "Firecracker binary",
        "jailer binary",
        "Firecracker seccomp filter",
        "`host-binaries.manifest.json`",
        "docs/behaviors/release/host-prerequisite-policy.md",
    ] {
        assert_contains(paragraph, required);
    }
    assert_contains_words(
        paragraph,
        "startup privilege through root, the documented m80 capability set, or a privileged container",
    );
    assert_contains_words(paragraph, "operator-provided host prerequisites");
}

#[test]
fn readme_and_policy_agree_on_common_host_prerequisite_story() {
    let readme = read_repo_file("README.md");
    let paragraph = readme_preflight_paragraph(&readme);
    let policy = read_repo_file("docs/behaviors/release/host-prerequisite-policy.md");

    for (readme_claim, policy_claim) in [
        (
            "Firecracker, jailer, and the Firecracker seccomp filter are operator-provided host prerequisites",
            "Firecracker, the official jailer, and Firecracker's seccomp filter as operator-provided host prerequisites",
        ),
        ("Linux/KVM host", "Install and launch assume a Linux/KVM host"),
        ("`/dev/kvm` access", "`/dev/kvm` present and writable"),
        (
            "startup privilege through root, the documented m80 capability set, or a privileged container",
            "startup privilege through root, the documented m80 capability set, or a privileged container",
        ),
        (
            "`host-binaries.manifest.json` for the installed host-side TCB",
            "root-owned, non-writable host TCB paths for Firecracker, jailer, m80 helpers, guest artifacts, and `host-binaries.manifest.json`",
        ),
    ] {
        assert_contains_words(paragraph, readme_claim);
        assert_contains_words(&policy, policy_claim);
    }
}
