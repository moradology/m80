use super::*;
use m80_snapshot_template::{
    GuestMountPath, ImageDigest, TemplateBodyPaths, TemplateRestoreLayout,
};

const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[test]
fn hostname_empty_is_rejected() {
    assert!(HostnameSpec::new("").is_err());
}

#[test]
fn hostname_total_length_254_is_rejected() {
    let hostname = format!("{}.{}", "a".repeat(126), "b".repeat(127));
    assert_eq!(hostname.len(), 254);
    assert!(HostnameSpec::new(&hostname).is_err());
}

#[test]
fn hostname_label_length_64_is_rejected() {
    assert!(HostnameSpec::new(&"a".repeat(64)).is_err());
}

#[test]
fn hostname_leading_hyphen_is_rejected() {
    assert!(HostnameSpec::new("-host").is_err());
}

#[test]
fn hostname_trailing_hyphen_is_rejected() {
    assert!(HostnameSpec::new("host-").is_err());
}

#[test]
fn hostname_non_ascii_is_rejected() {
    assert!(HostnameSpec::new("host\u{e9}").is_err());
}

#[test]
fn hostname_embedded_slash_is_rejected() {
    assert!(HostnameSpec::new("host/name").is_err());
}

#[test]
fn hostname_valid_rfc1123_name_is_accepted() {
    let hostname = HostnameSpec::new("lease-1.example").expect("valid hostname");
    assert_eq!(hostname.as_str(), "lease-1.example");
}

#[test]
fn hook_spec_set_digest_is_stable_for_same_order() {
    let hooks = vec![
        HookSpec::ReseedSystemdRandomSeed,
        HookSpec::SetHostname(HostnameSpec::new("lease-1").unwrap()),
        HookSpec::RegenMachineId,
    ];
    let first = HookSpecSet::new(hooks.clone());
    let second = HookSpecSet::new(hooks);
    assert_eq!(first.digest(), second.digest());
}

#[test]
fn hook_spec_set_digest_changes_when_order_changes() {
    let first = HookSpecSet::new(vec![
        HookSpec::ReseedSystemdRandomSeed,
        HookSpec::RegenMachineId,
    ]);
    let second = HookSpecSet::new(vec![
        HookSpec::RegenMachineId,
        HookSpec::ReseedSystemdRandomSeed,
    ]);
    assert_ne!(first.digest(), second.digest());
}

#[test]
fn template_fingerprint_sorts_pmem_entries_before_hashing() {
    let first = TemplateFingerprint::compute(&template_inputs(
        vec![pmem_entry("b"), pmem_entry("a")],
        HookSpecSet::empty(),
    ));
    let second = TemplateFingerprint::compute(&template_inputs(
        vec![pmem_entry("a"), pmem_entry("b")],
        HookSpecSet::empty(),
    ));
    assert_eq!(first, second);
}

#[test]
fn template_fingerprint_changes_when_hook_set_changes() {
    let first = TemplateFingerprint::compute(&template_inputs(
        vec![pmem_entry("a")],
        HookSpecSet::empty(),
    ));
    let second = TemplateFingerprint::compute(&template_inputs(
        vec![pmem_entry("a")],
        HookSpecSet::new(vec![HookSpec::RegenMachineId]),
    ));
    assert_ne!(first, second);
}

#[test]
fn jail_backing_path_rejects_relative_path() {
    assert!(JailBackingPath::parse("relative/path").is_err());
}

#[test]
fn jail_backing_path_rejects_parent_component() {
    assert!(JailBackingPath::parse("/snapshot/../escape").is_err());
}

#[test]
fn template_ref_carries_fingerprint() {
    let temp = tempfile::tempdir().expect("store temp");
    let store = TemplateStore::create(temp.path().join("templates"), 4).expect("store");
    let inputs = template_inputs(vec![pmem_entry("a")], HookSpecSet::empty());
    let fingerprint = TemplateFingerprint::compute(&inputs);
    let plan = store
        .reserve(inputs, restore_layout())
        .expect("reserve template");
    write_body(plan.body_paths());

    let pinned = store.commit(plan).expect("commit template");

    assert_eq!(pinned.reference().fingerprint(), &fingerprint);
}

fn template_inputs(
    pmem_image_digest_set: Vec<PmemTemplateEntry>,
    hook_spec_set: HookSpecSet,
) -> TemplateInputs {
    TemplateInputs::new(
        "6.17.0",
        "v1.15.1",
        TemplateDigest::parse(DIGEST_A).unwrap(),
        pmem_image_digest_set,
        TemplateDigest::parse(DIGEST_B).unwrap(),
        hook_spec_set,
    )
    .unwrap()
}

fn pmem_entry(name: &str) -> PmemTemplateEntry {
    PmemTemplateEntry::new(
        GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).unwrap(),
        ImageDigest::parse(DIGEST_C).unwrap(),
        PmemTemplateSharing::PerVm,
        JailBackingPath::parse(format!("/pmem/{name}.erofs")).unwrap(),
    )
}

fn restore_layout() -> TemplateRestoreLayout {
    TemplateRestoreLayout::new(
        JailBackingPath::parse("/snapshot/vm.snap").unwrap(),
        JailBackingPath::parse("/snapshot/mem.snap").unwrap(),
        Vec::new(),
    )
}

fn write_body(paths: &TemplateBodyPaths) {
    std::fs::write(&paths.vm_state, b"vm").expect("write vm");
    std::fs::write(&paths.mem, b"mem").expect("write mem");
}
