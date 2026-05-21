//! Release behavior integration tests.

#[path = "release/asset_index.rs"]
mod asset_index;

#[path = "release/direct_url_fixture_harness.rs"]
mod direct_url_fixture_harness;

#[path = "release/direct_url_diagnostics.rs"]
mod direct_url_diagnostics;

#[path = "release/installer_input_contract.rs"]
mod installer_input_contract;

#[path = "release/freshness_status_reader.rs"]
mod freshness_status_reader;

#[path = "release/install_handoff_identity.rs"]
mod install_handoff_identity;

#[path = "release/install_state_resolver.rs"]
mod install_state_resolver;

#[path = "release/release_tag_ordering.rs"]
mod release_tag_ordering;

#[path = "release/installer_layout.rs"]
mod installer_layout;

#[path = "release/release_integrity_material.rs"]
mod release_integrity_material;
