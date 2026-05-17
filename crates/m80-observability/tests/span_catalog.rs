use std::collections::BTreeSet;

use m80_observability::spans::{
    ALL_SPANS, M80_SPAN_GUEST_DAX_MOUNT, M80_SPAN_IMAGE_BUILD, M80_SPAN_PMEM_ATTACH,
    M80_SPAN_POST_RESTORE_HOOK, M80_SPAN_TEMPLATE_BUILD, M80_SPAN_TEMPLATE_RESTORE,
};

#[test]
fn span_catalog_is_non_empty_and_fielded() {
    assert!(!ALL_SPANS.is_empty());
    for spec in ALL_SPANS {
        assert!(!spec.name.is_empty(), "span name must not be empty");
        assert!(
            spec.name.starts_with("m80."),
            "span name should stay under m80 namespace: {}",
            spec.name
        );
        assert!(
            !spec.description.is_empty(),
            "span {} needs a description",
            spec.name
        );
        assert!(!spec.fields.is_empty(), "span {} needs fields", spec.name);
        assert!(
            !spec.emit_sites.is_empty(),
            "span {} needs emit-site docs",
            spec.name
        );
    }
}

#[test]
fn span_constants_are_unique_and_cataloged() {
    let constants = [
        M80_SPAN_IMAGE_BUILD,
        M80_SPAN_PMEM_ATTACH,
        M80_SPAN_GUEST_DAX_MOUNT,
        M80_SPAN_TEMPLATE_BUILD,
        M80_SPAN_TEMPLATE_RESTORE,
        M80_SPAN_POST_RESTORE_HOOK,
    ];

    let mut unique = BTreeSet::new();
    for name in constants {
        assert!(unique.insert(name), "duplicate span constant: {name}");
        assert!(
            ALL_SPANS.iter().any(|spec| spec.name == name),
            "constant missing from ALL_SPANS: {name}"
        );
    }
    assert_eq!(unique.len(), ALL_SPANS.len());
}
