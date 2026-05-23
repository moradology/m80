use std::fs;
use std::path::PathBuf;

const HUMAN_LABEL_LINT_ALLOW: &str = "m80-check-id-lint: allow-human-label-renderer";

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

fn collect_files(root: PathBuf, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read scan directory") {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            collect_files(path, files);
        } else {
            files.push(path);
        }
    }
}

fn host_prerequisite_consumer_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    for relative in ["crates/m80-cli/src", "crates/m80-preflight/src", "scripts"] {
        collect_files(root.join(relative), &mut files);
    }
    files
        .into_iter()
        .filter(|path| {
            let relative = path.strip_prefix(&root).expect("path under repo root");
            let relative_text = relative.to_string_lossy();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            if relative_text.contains("/tests/") || name.starts_with("test-") {
                return false;
            }
            if relative.starts_with("scripts") {
                return name.ends_with(".py")
                    && [
                        "freshness",
                        "install",
                        "preflight",
                        "proof",
                        "quickstart",
                        "release",
                    ]
                    .iter()
                    .any(|needle| name.contains(needle));
            }
            matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs")
            )
        })
        .collect()
}

fn is_machine_decision_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let condition_like = trimmed.starts_with("if ")
        || trimmed.starts_with("match ")
        || trimmed.starts_with("while ")
        || line.contains(".find(")
        || line.contains(".any(")
        || line.contains(".all(")
        || line.contains(".filter(")
        || line.contains(".position(")
        || line.contains(".contains(");
    condition_like
        && (line.contains("==")
            || line.contains("!=")
            || line.contains(".contains(")
            || line.contains(" in "))
}

fn dangerous_human_label_lookup(line: &str) -> Option<&'static str> {
    if line.contains(HUMAN_LABEL_LINT_ALLOW) {
        return None;
    }
    if line.contains(".label") && is_machine_decision_line(line) {
        return Some("CheckRow.label");
    }
    if (line.contains(".check_name")
        || line.contains("[\"check_name\"]")
        || line.contains(".get(\"check_name\")")
        || line.contains("'check_name'"))
        && is_machine_decision_line(line)
    {
        return Some("HostPrerequisiteCheck.check_name");
    }
    None
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
    assert_contains_words(
        &policy,
        "`scripts/verify-release-bundle.py` rejects tar entries such as `bin/firecracker`, `bin/jailer`, and `bin/firecracker-seccomp-filter.bin`",
    );
    assert_contains_words(
        &policy,
        "`m80 quickstart` rejects the same payload names before creating or changing the active artifact directory.",
    );
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
fn policy_doc_names_host_prerequisite_repair_token_catalog() {
    let policy = read_repo_file("docs/behaviors/release/host-prerequisite-policy.md");

    assert_contains(&policy, "## Repair Token Catalog");
    for required in [
        "`install-firecracker-prerequisites`",
        "`repair-kvm`",
        "`repair-cgroup-mode`",
        "`repair-privilege`",
        "`upgrade-firecracker-cve-floor`",
        "`repair-host-binaries-manifest`",
        "`repair-host-setup`",
        "`reinstall-m80-release`",
        "`docs/behaviors/release/host-prerequisite-policy.md`",
        "`docs/ops/host-setup.md`",
        "`docs/ops/binary-installation.md`",
        "`docs/security/firecracker-cve-floor.md`",
    ] {
        assert_contains(&policy, required);
    }
    assert_contains_words(
        &policy,
        "`reinstall-m80-release` is the only token that carries an installer command",
    );
}

#[test]
fn policy_doc_requires_preflight_proof_checks_for_release_artifacts() {
    let policy = read_repo_file("docs/behaviors/release/host-prerequisite-policy.md");

    assert_contains(&policy, "`m80 preflight --json`");
    assert_contains(&policy, "`data.host_prerequisite_failure`");
    assert_contains_words(
        &policy,
        "`Firecracker binary` check records the expected and observed Firecracker version",
    );
    assert_contains_words(
        &policy,
        "`Jailer binary` check records the expected jailer version and the observed jailer version",
    );
    assert_contains_words(
        &policy,
        "operator-provided train matches the guest artifact set",
    );
}

#[test]
fn release_runbook_uses_host_prerequisite_result_contract() {
    let runbook = read_repo_file("docs/runbook/release.md");

    assert_contains(&runbook, "`HostPrerequisiteResult`");
    assert_contains_words(
        &runbook,
        "`Firecracker binary` check whose expected and observed version fields record the Firecracker version",
    );
    assert_contains_words(
        &runbook,
        "`Jailer binary` check whose expected and observed version fields record the jailer version",
    );
}

#[test]
fn verifier_doc_records_stable_check_id_registry() {
    let doc = read_repo_file("docs/behaviors/preflight/host-prerequisite-verifier.md");

    assert_contains(&doc, "`check_id`, the stable machine identity");
    assert_contains(&doc, "optional expected/actual scalar values");
    assert_contains(
        &doc,
        "pins m80-owned helper repairs to that exact `install.sh`",
    );
    assert_contains_words(
        &doc,
        "Machine consumers key on `check_id`, not `check_name`",
    );
    assert_contains_words(
        &doc,
        "Production machine consumers are linted against decisions keyed on `CheckRow.label` or `HostPrerequisiteCheck.check_name`",
    );
    assert_contains(&doc, HUMAN_LABEL_LINT_ALLOW);
    for required in [
        "`os_gate`",
        "`kvm`",
        "`cgroup_mode`",
        "`privilege`",
        "`firecracker_binary`",
        "`firecracker_seccomp_filter`",
        "`jailer_binary`",
        "`host_binary_manifest`",
        "`rootfs_manifest`",
    ] {
        assert_contains(&doc, required);
    }
}

#[test]
fn machine_consumers_do_not_key_on_human_preflight_labels() {
    let root = repo_root();
    let mut violations = Vec::new();

    for path in host_prerequisite_consumer_files() {
        let text = fs::read_to_string(&path).expect("read scanned source file");
        for (index, line) in text.lines().enumerate() {
            let Some(kind) = dangerous_human_label_lookup(line) else {
                continue;
            };
            let relative = path.strip_prefix(&root).expect("path under repo root");
            violations.push(format!(
                "{}:{}: machine decision uses {kind}; key on check_id/HostPrerequisiteCheckId instead: {}",
                relative.display(),
                index + 1,
                line.trim()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "host prerequisite machine consumers must use check_id, not human labels:\n{}",
        violations.join("\n")
    );
}

#[test]
fn readme_links_host_prerequisite_docs_only_from_preflight_remediation_context() {
    let readme = read_repo_file("README.md");
    let links = [
        "docs/behaviors/release/host-prerequisite-policy.md",
        "docs/behaviors/preflight/host-prerequisite-verifier.md",
    ];

    let (preflight_index, _) = readme_preflight_paragraph_bounds(&readme);
    let diagnostics_index = readme
        .find("## Diagnostics")
        .expect("README should have diagnostics section");

    for link in links {
        let link_count = readme.match_indices(link).count();
        assert_eq!(
            link_count, 1,
            "README should link {link} once from remediation context"
        );

        let link_index = readme.find(link).expect("README should link host doc");
        assert!(
            preflight_index < link_index && link_index < diagnostics_index,
            "{link} must stay in the quickstart preflight/remediation paragraph"
        );
    }
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
        "docs/behaviors/preflight/host-prerequisite-verifier.md",
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
