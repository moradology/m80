//! Release behavior integration tests.

#[path = "release/asset_index.rs"]
mod asset_index;

#[path = "release/direct_url_fixture_harness.rs"]
mod direct_url_fixture_harness;

#[path = "release/direct_url_diagnostics.rs"]
mod direct_url_diagnostics;

#[path = "release/downgrade_refusal.rs"]
mod downgrade_refusal;

#[path = "release/installer_input_contract.rs"]
mod installer_input_contract;

#[path = "release/freshness_status_reader.rs"]
mod freshness_status_reader;

#[path = "release/freshness_failure_policy.rs"]
mod freshness_failure_policy;

#[path = "release/current_latest_repair_preflight.rs"]
mod current_latest_repair_preflight;

#[path = "release/current_latest_repair_release_note.rs"]
mod current_latest_repair_release_note;

#[path = "release/current_latest_version_cutover.rs"]
mod current_latest_version_cutover;

#[path = "release/install_handoff_identity.rs"]
mod install_handoff_identity;

#[path = "release/install_state_resolver.rs"]
mod install_state_resolver;

#[path = "release/install_transaction_contract.rs"]
mod install_transaction_contract;

#[path = "release/mismatch_diagnostics.rs"]
mod mismatch_diagnostics;

#[path = "release/install_cleanup.rs"]
mod install_cleanup;

#[path = "release/publication_plan.rs"]
mod publication_plan;

#[path = "release/public_access_receipt.rs"]
mod public_access_receipt;

#[path = "release/release_tag_ordering.rs"]
mod release_tag_ordering;

#[path = "release/safety_floor_schema.rs"]
mod safety_floor_schema;

#[path = "release/installer_layout.rs"]
mod installer_layout;

#[path = "release/release_integrity_material.rs"]
mod release_integrity_material;

#[path = "release/update_check.rs"]
mod update_check;
