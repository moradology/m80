use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::super::test_env::valid_material;
use super::super::{
    verify, verify_support, ReleaseMaterialPlan, ASSET_INDEX_NAME, BOOTSTRAP_SELECTOR_NAME,
    INSTALL_SCRIPT_NAME, PUBLIC_SHA256SUMS_NAME, RELEASE_ATTESTATION_BUNDLE_NAME,
    RELEASE_ATTESTATION_METADATA_NAME, RELEASE_BUILD_NAME, RELEASE_INTEGRITY_NAME,
};

#[test]
fn direct_release_material_plan_matches_shared_integrity_contract() {
    let contract = release_integrity_contract();
    let plan = ReleaseMaterialPlan::from_index_material(valid_material()).unwrap();

    assert_eq!(
        contract["schema_version"],
        json!(verify::RELEASE_INTEGRITY_SCHEMA_VERSION)
    );
    assert_eq!(
        contract["mechanism"],
        json!(verify::RELEASE_INTEGRITY_MECHANISM)
    );
    assert_eq!(
        contract["repository"],
        json!(crate::release_urls::release_repository())
    );
    assert_eq!(contract["target"], json!("linux-x86_64"));
    assert_eq!(
        contract["default_bundle"]["name"],
        json!("m80-linux-x86_64.tar.gz")
    );
    assert_eq!(
        contract["default_bundle"]["checksum_name"],
        json!("m80-linux-x86_64.tar.gz.sha256")
    );
    assert_eq!(
        contract["default_bundle"]["metadata_name"],
        json!("m80-linux-x86_64.bundle.json")
    );
    assert_eq!(
        contract["default_bundle"]["metadata_checksum_name"],
        json!("m80-linux-x86_64.bundle.json.sha256")
    );
    assert_eq!(
        contract["required_files"]["asset_index"],
        json!(ASSET_INDEX_NAME)
    );
    assert_eq!(
        contract["required_files"]["bootstrap_selector"],
        json!(BOOTSTRAP_SELECTOR_NAME)
    );
    assert_eq!(
        contract["required_files"]["build_manifest"],
        json!(RELEASE_BUILD_NAME)
    );
    assert_eq!(
        contract["required_files"]["install"],
        json!(INSTALL_SCRIPT_NAME)
    );
    assert_eq!(
        contract["required_files"]["integrity_predicate"],
        json!(RELEASE_INTEGRITY_NAME)
    );
    assert_eq!(
        contract["required_files"]["public_sha256s"],
        json!(PUBLIC_SHA256SUMS_NAME)
    );
    assert_eq!(
        contract["attestation"]["bundle_name"],
        json!(RELEASE_ATTESTATION_BUNDLE_NAME)
    );
    assert_eq!(
        contract["attestation"]["metadata_name"],
        json!(RELEASE_ATTESTATION_METADATA_NAME)
    );
    assert_eq!(
        contract["attestation"]["signer_workflow"],
        json!(verify::RELEASE_ATTESTATION_SIGNER_WORKFLOW)
    );
    assert_eq!(
        contract["attestation"]["issuer"],
        json!(verify::RELEASE_ATTESTATION_ISSUER)
    );
    assert_eq!(
        contract["attestation"]["keyset_id"],
        json!(verify::RELEASE_ATTESTATION_KEYSET_ID)
    );

    let actual_roles = plan
        .materials
        .iter()
        .map(|material| {
            let subject_kind = verify_support::material_subject_kind(material);
            json!({
                "class": material.class,
                "name": material.name.as_str(),
                "public_sha256s": subject_kind.is_some() && material.name != PUBLIC_SHA256SUMS_NAME,
                "subject_kind": subject_kind,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(contract["material_roles"], json!(actual_roles));
}

fn release_integrity_contract() -> Value {
    let path = repo_root().join("docs/behaviors/release/release-integrity-contract.json");
    let raw = fs::read_to_string(&path).expect("read release integrity contract");
    serde_json::from_str(&raw).expect("parse release integrity contract")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("m80-cli crate should live under crates/")
        .to_path_buf()
}
