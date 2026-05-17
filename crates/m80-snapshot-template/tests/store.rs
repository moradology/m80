use std::fs;
use std::thread;
use std::time::Duration;

use m80_snapshot_template::{
    GuestMountPath, HookSpec, HookSpecSet, HostnameSpec, ImageDigest, Index, JailBackingPath,
    PmemTemplateEntry, PmemTemplateSharing, TemplateDigest, TemplateFingerprint,
    TemplateRestoreLayout, TemplateStore, TemplateStoreError,
};

#[test]
fn cache_miss_reserve_commit_then_lookup_returns_pinned_template() {
    let fixture = Fixture::new(4);
    let inputs = template_inputs("v1.15.1", "b");

    assert!(fixture.store.lookup(&inputs).expect("lookup").is_none());
    let plan = fixture
        .store
        .reserve(inputs.clone(), restore_layout())
        .expect("reserve");
    let fingerprint = *plan.fingerprint();
    assert!(
        !fixture.store.template_dir(&fingerprint).exists(),
        "reserve must not create a visible content-addressed template"
    );

    write_body(&plan, "a");
    let pinned = fixture.store.commit(plan).expect("commit");

    assert_eq!(pinned.reference().fingerprint(), &fingerprint);
    assert_eq!(
        pinned.body_paths().vm_state,
        fixture.store.template_dir(&fingerprint).join("vm.snap")
    );
    assert_eq!(pinned.manifest().fingerprint, fingerprint);
    assert_eq!(pinned.manifest().snapshot_manifest.artifacts.len(), 2);
    assert_artifact_path(
        pinned.manifest(),
        m80_snapshot::ArtifactKind::Memory,
        &pinned.body_paths().mem,
    );
    assert_artifact_path(
        pinned.manifest(),
        m80_snapshot::ArtifactKind::VmState,
        &pinned.body_paths().vm_state,
    );
    let sidecar =
        m80_snapshot::verify_snapshot_manifest(&pinned.body_paths().snapshot_paths(), "v1.15.1")
            .expect("snapshot sidecar verifies");
    assert_eq!(sidecar, pinned.manifest().snapshot_manifest);
    drop(pinned);

    let hit = fixture.store.lookup(&inputs).expect("lookup hit");
    assert!(hit.is_some(), "committed template should be a cache hit");
}

#[test]
fn captured_staging_body_is_not_visible_until_commit() {
    let fixture = Fixture::new(4);
    let inputs = template_inputs("v1.15.1", "b");
    let plan = fixture
        .store
        .reserve(inputs.clone(), restore_layout())
        .expect("reserve");
    let fingerprint = *plan.fingerprint();

    write_body(&plan, "captured");

    assert!(
        !fixture.store.template_dir(&fingerprint).exists(),
        "captured staging files must not publish a visible template"
    );
    assert!(
        fixture.store.lookup(&inputs).expect("lookup").is_none(),
        "lookup must remain a miss until commit"
    );
    drop(plan);
    assert!(
        fixture
            .store
            .lookup(&inputs)
            .expect("lookup after drop")
            .is_none(),
        "dropping a captured-but-uncommitted plan must still leave a miss"
    );
}

#[test]
fn commit_missing_body_file_leaves_no_visible_template() {
    let fixture = Fixture::new(4);
    let inputs = template_inputs("v1.15.1", "c");
    let plan = fixture
        .store
        .reserve(inputs, restore_layout())
        .expect("reserve");
    let fingerprint = *plan.fingerprint();
    fs::write(&plan.body_paths().vm_state, b"vm-only").expect("write vm");

    let err = fixture
        .store
        .commit(plan)
        .expect_err("missing mem must fail");

    assert!(
        matches!(err, TemplateStoreError::MissingBody { ref path } if path.ends_with("mem.snap")),
        "expected missing mem.snap, got {err:?}"
    );
    assert!(
        !fixture.store.template_dir(&fingerprint).exists(),
        "failed commit must not publish a visible template"
    );
}

#[test]
fn lru_eviction_skips_process_pinned_template() {
    let fixture = Fixture::new(2);
    let pinned_a = commit_template(&fixture.store, template_inputs("v1.15.1", "d"), "a");
    let fingerprint_a = *pinned_a.reference().fingerprint();
    thread::sleep(Duration::from_millis(2));

    let pinned_b = commit_template(&fixture.store, template_inputs("v1.15.1", "e"), "b");
    let fingerprint_b = *pinned_b.reference().fingerprint();
    drop(pinned_b);
    thread::sleep(Duration::from_millis(2));

    let pinned_c = commit_template(&fixture.store, template_inputs("v1.15.1", "f"), "c");
    let fingerprint_c = *pinned_c.reference().fingerprint();

    assert!(fixture.store.template_dir(&fingerprint_a).exists());
    assert!(
        !fixture.store.template_dir(&fingerprint_b).exists(),
        "oldest unpinned template should be evicted"
    );
    assert!(fixture.store.template_dir(&fingerprint_c).exists());
}

#[test]
fn fingerprint_mismatch_is_fail_closed_and_invalidated_template_can_be_evicted() {
    let fixture = Fixture::new(4);
    let inputs = template_inputs("v1.15.1", "1");
    let pinned = commit_template(&fixture.store, inputs, "a");
    let fingerprint = *pinned.reference().fingerprint();
    drop(pinned);

    let bumped = template_inputs("v1.16.0", "1");
    let live = TemplateFingerprint::compute(&bumped);
    let err = fixture
        .store
        .pin(&fingerprint, &bumped)
        .expect_err("version bump must invalidate old template");

    assert!(
        matches!(err, TemplateStoreError::FingerprintMismatch { stored, live: got } if stored == fingerprint && got == live),
        "expected fingerprint mismatch, got {err:?}"
    );
    let removed = fixture
        .store
        .evict_invalidated(&bumped)
        .expect("evict invalidated");
    assert_eq!(removed, 1);
    assert!(!fixture.store.template_dir(&fingerprint).exists());
}

#[test]
fn scoped_invalidation_preserves_unrelated_template_families() {
    let fixture = Fixture::new(8);
    let stale_same_family = commit_template(
        &fixture.store,
        template_inputs_with_pmem("v1.15.1", "1", "c"),
        "stale-same-family",
    );
    let stale_same_family_fingerprint = *stale_same_family.reference().fingerprint();
    drop(stale_same_family);
    let different_post_init = commit_template(
        &fixture.store,
        template_inputs_with_pmem("v1.15.1", "2", "c"),
        "different-post-init",
    );
    let different_post_init_fingerprint = *different_post_init.reference().fingerprint();
    drop(different_post_init);
    let different_pmem = commit_template(
        &fixture.store,
        template_inputs_with_pmem("v1.15.1", "1", "d"),
        "different-pmem",
    );
    let different_pmem_fingerprint = *different_pmem.reference().fingerprint();
    drop(different_pmem);

    let live = template_inputs_with_pmem("v1.16.0", "1", "c");
    let removed = fixture
        .store
        .evict_invalidated_in_scope(&live)
        .expect("evict scoped invalidated");

    assert_eq!(removed, 1);
    assert!(!fixture
        .store
        .template_dir(&stale_same_family_fingerprint)
        .exists());
    assert!(
        fixture
            .store
            .template_dir(&different_post_init_fingerprint)
            .exists(),
        "different post-init family must not be pruned"
    );
    assert!(
        fixture
            .store
            .template_dir(&different_pmem_fingerprint)
            .exists(),
        "different pmem family must not be pruned"
    );
}

#[test]
fn list_and_image_reference_query_read_committed_manifests() {
    let fixture = Fixture::new(4);
    let first = commit_template(
        &fixture.store,
        template_inputs_with_pmem("v1.15.1", "1", "c"),
        "first",
    );
    let first_fingerprint = *first.reference().fingerprint();
    drop(first);
    let second = commit_template(
        &fixture.store,
        template_inputs_with_pmem("v1.15.1", "2", "d"),
        "second",
    );
    drop(second);

    let summaries = fixture.store.list().expect("list templates");
    assert_eq!(summaries.len(), 2);
    assert!(summaries
        .iter()
        .any(|summary| summary.fingerprint == first_fingerprint && summary.size_bytes > 0));

    let referencing = fixture
        .store
        .templates_referencing_image(&image_digest("c"))
        .expect("references");
    assert_eq!(referencing, vec![first_fingerprint]);
    assert!(
        fixture
            .store
            .templates_referencing_image(&image_digest("e"))
            .expect("missing references")
            .is_empty(),
        "unreferenced image digest should return no templates"
    );
}

#[test]
fn manifest_and_remove_operate_on_unpinned_templates() {
    let fixture = Fixture::new(4);
    let pinned = commit_template(&fixture.store, template_inputs("v1.15.1", "1"), "a");
    let fingerprint = *pinned.reference().fingerprint();

    let err = fixture
        .store
        .remove(&fingerprint)
        .expect_err("pinned template must not be removed");
    assert!(
        matches!(err, TemplateStoreError::TemplatePinned { .. }),
        "expected pinned error, got {err:?}"
    );
    drop(pinned);

    let manifest = fixture.store.manifest(&fingerprint).expect("manifest");
    assert_eq!(manifest.fingerprint, fingerprint);

    fixture.store.remove(&fingerprint).expect("remove template");
    assert!(!fixture.store.template_dir(&fingerprint).exists());
    assert!(fixture.store.list().expect("list after remove").is_empty());
    assert!(matches!(
        fixture.store.manifest(&fingerprint),
        Err(TemplateStoreError::TemplateMissing { .. })
    ));
}

#[test]
fn index_schema_probe_reports_future_version_before_unknown_fields() {
    let err = Index::from_bytes(br#"{"schema_version":2,"future_field":true}"#)
        .expect_err("future schema should be rejected by version");

    assert!(
        matches!(
            err,
            TemplateStoreError::UnsupportedSchemaVersion {
                got: 2,
                expected: 1
            }
        ),
        "expected UnsupportedSchemaVersion, got {err:?}"
    );
}

#[test]
fn index_denies_unknown_fields_for_current_schema() {
    let err = Index::from_bytes(br#"{"schema_version":1,"entries":[],"extra":true}"#)
        .expect_err("unknown current-schema field should fail");

    assert!(
        matches!(err, TemplateStoreError::Json { .. }),
        "expected JSON unknown-field error, got {err:?}"
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    store: TemplateStore,
}

impl Fixture {
    fn new(capacity: usize) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("templates");
        let store = TemplateStore::create(&root, capacity).expect("create store");
        Self { _temp: temp, store }
    }
}

fn commit_template(
    store: &TemplateStore,
    inputs: m80_snapshot_template::TemplateInputs,
    body: &str,
) -> m80_snapshot_template::PinnedTemplate {
    let plan = store
        .reserve(inputs, restore_layout())
        .expect("reserve template");
    write_body(&plan, body);
    store.commit(plan).expect("commit template")
}

fn write_body(plan: &m80_snapshot_template::TemplateBuildPlan, body: &str) {
    fs::write(&plan.body_paths().vm_state, format!("vm-{body}")).expect("write vm");
    fs::write(&plan.body_paths().mem, format!("mem-{body}")).expect("write mem");
}

fn template_inputs(
    firecracker_version: &str,
    post_digest_byte: &str,
) -> m80_snapshot_template::TemplateInputs {
    template_inputs_with_pmem(firecracker_version, post_digest_byte, "c")
}

fn template_inputs_with_pmem(
    firecracker_version: &str,
    post_digest_byte: &str,
    image_digest_byte: &str,
) -> m80_snapshot_template::TemplateInputs {
    m80_snapshot_template::TemplateInputs::new(
        "6.17.0",
        firecracker_version,
        digest("a"),
        vec![pmem_entry("toolchain", image_digest_byte)],
        digest(post_digest_byte),
        HookSpecSet::new(vec![
            HookSpec::ReseedSystemdRandomSeed,
            HookSpec::SetHostname(HostnameSpec::new("lease-1").expect("hostname")),
        ]),
    )
    .expect("template inputs")
}

fn restore_layout() -> TemplateRestoreLayout {
    TemplateRestoreLayout::new(
        JailBackingPath::parse("/snapshot/vm.snap").expect("vm state jail path"),
        JailBackingPath::parse("/snapshot/mem.snap").expect("mem jail path"),
        vec![pmem_entry("toolchain", "c")],
    )
}

fn pmem_entry(name: &str, image_digest_byte: &str) -> PmemTemplateEntry {
    PmemTemplateEntry::new(
        GuestMountPath::parse(&format!("/opt/m80-layers/{name}")).expect("mount path"),
        image_digest(image_digest_byte),
        PmemTemplateSharing::Shared,
        JailBackingPath::parse(format!("/pmem/{name}.erofs")).expect("jail backing path"),
    )
}

fn digest(byte: &str) -> TemplateDigest {
    TemplateDigest::parse(&byte.repeat(64)).expect("digest")
}

fn image_digest(byte: &str) -> ImageDigest {
    ImageDigest::parse(&byte.repeat(64)).expect("image digest")
}

fn assert_artifact_path(
    manifest: &m80_snapshot_template::TemplateManifest,
    kind: m80_snapshot::ArtifactKind,
    expected: &std::path::Path,
) {
    let artifact = manifest
        .snapshot_manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == kind)
        .expect("artifact kind");
    assert_eq!(artifact.path, expected);
}
