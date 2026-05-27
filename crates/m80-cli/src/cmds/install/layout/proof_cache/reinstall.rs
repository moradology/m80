use std::path::Path;

use m80_firecracker::{ConfigError, FcError};
use serde::Serialize;

use super::{
    read_proof_cache_manifest, verified_release_proof_cache_manifest, verify_cached_payload_files,
    verify_dir_mode, verify_file_mode, verify_payload_file_modes, ProofCachePayload,
    VerifiedOfficialReleaseBundle, PROOF_CACHE_DIR, PROOF_CACHE_MANIFEST,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProofCacheReinstallReport {
    pub(crate) status: &'static str,
    pub(crate) existing_manifest_digest: String,
    pub(crate) verified_manifest_digest: String,
}

pub(crate) fn compare_existing_release_proof_cache(
    verified: &VerifiedOfficialReleaseBundle,
    final_dir: &Path,
    m80_version: &str,
    repair_command: &str,
) -> Result<ProofCacheReinstallReport, FcError> {
    let verified_manifest = verified_release_proof_cache_manifest(verified, m80_version)?;
    let cache_dir = final_dir.join("artifacts").join(PROOF_CACHE_DIR);
    let existing_manifest_path = cache_dir.join(PROOF_CACHE_MANIFEST);
    let existing_manifest = read_proof_cache_manifest(&existing_manifest_path).map_err(|err| {
        proof_cache_reinstall_invalid(
            final_dir,
            "installed proof-cache manifest is unreadable or invalid",
            err,
            repair_command,
        )
    })?;
    verify_cached_payload_files(&cache_dir, &existing_manifest.payload).map_err(|err| {
        proof_cache_reinstall_invalid(
            final_dir,
            "installed proof-cache material is stale",
            err,
            repair_command,
        )
    })?;
    verify_dir_mode(&cache_dir).map_err(|err| {
        proof_cache_reinstall_invalid(
            final_dir,
            "installed proof-cache directory mode is stale",
            err,
            repair_command,
        )
    })?;
    verify_file_mode(&existing_manifest_path).map_err(|err| {
        proof_cache_reinstall_invalid(
            final_dir,
            "installed proof-cache manifest mode is stale",
            err,
            repair_command,
        )
    })?;
    verify_payload_file_modes(&cache_dir, &existing_manifest.payload).map_err(|err| {
        proof_cache_reinstall_invalid(
            final_dir,
            "installed proof-cache material mode is stale",
            err,
            repair_command,
        )
    })?;

    let changed_fields =
        public_material_mismatch_fields(&existing_manifest.payload, &verified_manifest.payload);
    if !changed_fields.is_empty() {
        return Err(proof_cache_reinstall_changed(
            final_dir,
            &existing_manifest.manifest_digest,
            &verified_manifest.manifest_digest,
            &changed_fields,
            repair_command,
        ));
    }

    Ok(ProofCacheReinstallReport {
        status: "idempotent_same_material",
        existing_manifest_digest: existing_manifest.manifest_digest,
        verified_manifest_digest: verified_manifest.manifest_digest,
    })
}

fn public_material_mismatch_fields(
    existing: &ProofCachePayload,
    verified: &ProofCachePayload,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    push_changed(
        &mut fields,
        "release_tag",
        &existing.release_tag,
        &verified.release_tag,
    );
    push_changed(
        &mut fields,
        "repository",
        &existing.repository,
        &verified.repository,
    );
    push_changed(&mut fields, "target", &existing.target, &verified.target);
    push_changed(
        &mut fields,
        "integrity_predicate",
        &existing.integrity_predicate,
        &verified.integrity_predicate,
    );
    push_changed(
        &mut fields,
        "attestation_bundle",
        &existing.attestation_bundle,
        &verified.attestation_bundle,
    );
    push_changed(
        &mut fields,
        "attestation_metadata.file",
        &existing.attestation_metadata.file,
        &verified.attestation_metadata.file,
    );
    push_changed(
        &mut fields,
        "attestation_metadata.signer_identity",
        &existing.attestation_metadata.signer_identity,
        &verified.attestation_metadata.signer_identity,
    );
    push_changed(
        &mut fields,
        "attestation_metadata.issuer",
        &existing.attestation_metadata.issuer,
        &verified.attestation_metadata.issuer,
    );
    push_changed(
        &mut fields,
        "attestation_metadata.keyset_id",
        &existing.attestation_metadata.keyset_id,
        &verified.attestation_metadata.keyset_id,
    );
    push_changed(
        &mut fields,
        "attestation_metadata.predicate_sha256",
        &existing.attestation_metadata.predicate_sha256,
        &verified.attestation_metadata.predicate_sha256,
    );
    push_changed(
        &mut fields,
        "asset_index",
        &existing.asset_index,
        &verified.asset_index,
    );
    push_changed(
        &mut fields,
        "trust_policy.path",
        &existing.trust_policy.path,
        &verified.trust_policy.path,
    );
    push_changed(
        &mut fields,
        "trust_policy.identity",
        &existing.trust_policy.identity,
        &verified.trust_policy.identity,
    );
    push_changed(
        &mut fields,
        "trust_policy.sha256",
        &existing.trust_policy.sha256,
        &verified.trust_policy.sha256,
    );
    fields
}

fn push_changed<T: PartialEq>(
    fields: &mut Vec<&'static str>,
    field: &'static str,
    existing: &T,
    verified: &T,
) {
    if existing != verified {
        fields.push(field);
    }
}

fn proof_cache_reinstall_changed(
    final_dir: &Path,
    existing_manifest_digest: &str,
    verified_manifest_digest: &str,
    changed_fields: &[&'static str],
    repair_command: &str,
) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "proof-cache.reinstall",
        reason: format!(
            "same-version reinstall would replace verified trust material; refusing silent replacement: existing_manifest_digest={existing_manifest_digest} verified_manifest_digest={verified_manifest_digest} changed_fields={} version_dir={} explicit_repair=review_changed_public_material_then_remove_version_dir_and_reinstall repair_command={repair_command}",
            changed_fields.join(","),
            final_dir.display()
        ),
    })
}

fn proof_cache_reinstall_invalid(
    final_dir: &Path,
    detail: &'static str,
    source: FcError,
    repair_command: &str,
) -> FcError {
    FcError::Config(ConfigError::InvalidValue {
        field: "proof-cache.reinstall",
        reason: format!(
            "same-version reinstall found stale proof cache; {detail}: source={source}; version_dir={} repair_command={repair_command}",
            final_dir.display()
        ),
    })
}
