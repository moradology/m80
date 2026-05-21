# Downgrade Refusal

Bead: `m80-o3uh9.16.12.3`.

Normal install and future update paths refuse to move an active install from a
newer stable release to an older stable release. The default rule is
fail-closed: `<install-root>/active` is read before asset-index fetch,
attestation verifier preflight, installer staging, profile writes, proof-cache
writes, or active-pointer replacement.

The comparison only uses concrete stable tags with shape
`vMAJOR.MINOR.PATCH`. A target equal to the active tag is not a downgrade. A
newer target is eligible to continue into the normal verification and install
transaction. An older target returns the structured release-transition code
`downgrade_refused` and leaves install state untouched.

Human diagnostics include:

- `release_transition_code=downgrade_refused`;
- `active_tag=<tag>`;
- `requested_tag=<tag>`;
- `expected_ordering=<policy text>`;
- `observed_ordering=target_older`;
- `reinstall_active_command=<pinned active install.sh command>`.

JSON diagnostics use the `ReleaseTransition` variant with `code`,
`active_tag`, `requested_tag`, `expected_ordering`, `observed_ordering`,
`reinstall_active_command`, and `rollback_command`. `rollback_command` is null
until m80 ships an explicit rollback command; accidental downgrade does not get
an implicit override.

If the active pointer is missing, the policy treats the operation as a first
install and does not block the target. If the active pointer names a stable tag
but profile or proof metadata is missing, the pointer tag is still authoritative
for downgrade refusal so stale metadata cannot bypass the active-release guard.

Tests:

- `release_tag_source_refuses_downgrade_before_index_fetch`;
- `official_bundle_url_refuses_downgrade_before_attestation_preflight`;
- `release_transition_json_payload_carries_downgrade_tags`;
- `same_version_reinstall_is_not_downgrade_refused`;
- `missing_active_metadata_still_refuses_older_target_by_pointer_tag`;
- `missing_active_pointer_does_not_block_first_install`;
- `release_tag_source_rejects_prerelease_tag_before_index_fetch`;
- `bootstrap_tag_source_rejects_prerelease_tag_before_index_fetch`;
- `downgrade_refusal_doc_names_policy_and_json_contract`.
