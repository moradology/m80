# Install State

Behavior beads: `m80-o3uh9.16.8.1`, `m80-o3uh9.16.8.2`,
`m80-o3uh9.16.8.3`, `m80-o3uh9.16.7.1`,
`m80-o3uh9.16.7.2`, `m80-o3uh9.16.7.3`,
`m80-o3uh9.16.7.4`, `m80-o3uh9.16.7.5`,
`m80-o3uh9.16.7.6`, `m80-o3uh9.16.8.4`,
`m80-o3uh9.16.11`, `m80-o3uh9.16.12.6`,
`m80-o3uh9.16.12.7`.

Installed state is rooted under one versioned directory,
`<install-root>/versions/<release_tag>`:

```text
<install-root>/versions/<release_tag>/
```

The active install is selected by `<install-root>/active`, an absolute symlink
that points at one versioned directory. Status readers treat the active pointer
as the local install selector; they do not infer trust from transient download
directories, workflow logs, or current GitHub release pages.

## Upgrade And Downgrade Policy

Release installs publish immutable versioned directories under
`<install-root>/versions/<release_tag>`. A mutating install verifies the target
release in a private staging tree, writes installed metadata and proof material
inside that staged tree, publishes the version directory, updates the generated
profile/config only through the install transaction, and flips
`<install-root>/active` last.

A successful upgrade to a newer stable release preserves the previous active
version directory. That previous directory is the only rollback target the
operator should use: rollback is an explicit active-pointer move to an
already-installed version, followed by `m80 install-status`. Installing an older
stable release is not rollback; normal install and update paths refuse it by
default with `downgrade_refused` and record bounded attempt metadata in
`<install-root>/last-install-attempt.json`.

| Outcome | Mutation behavior | Repair command |
| --- | --- | --- |
| First install or newer verified release | Stage and verify target, publish `<install-root>/versions/<release_tag>`, update generated selector files, then flip `<install-root>/active` last. | `m80 run -- echo hello` after status is healthy. |
| Failed verification or failed finalization | Leave previous active pointer, generated profile, config, proof cache, and version selection unchanged. | Retry the same pinned `curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh \| sudo sh` command after fixing the named failure. |
| Older target without explicit rollback | Refuse before active-state mutation; do not stage, rewrite profiles, or flip `<install-root>/active`. | Reinstall the current active release with its pinned `install.sh` command, or choose the explicit rollback path below. |
| Explicit rollback to an already-installed version | Operator moves only `<install-root>/active` to `<install-root>/versions/<previous-tag>`; m80 does not fetch, rewrite, or bless bytes. | `sudo ln -sfnT -- '<install-root>/versions/<previous-tag>' '<install-root>/active'` then `m80 install-status`. |
| Operator profile/config override | Report `explicit_override`; do not treat the override as installed-default proof. | Remove the override, or pass the same explicit profile/root to status and support captures. |

## Active Install Resolver

The resolver for `m80-o3uh9.16.7.2` is a read-only local status primitive. It
reads `<install-root>/active`, the selected version directory, effective
`default_profile` config, and the selected runtime profile. It does not execute
installed binaries, does not run preflight, does not fetch release metadata, and
does not download bundles.

For the default install root `/opt/m80`, effective config/profile lookup uses
the host selector paths (`/etc/m80/config.toml` and
`/etc/m80/profiles/<name>.toml`) because those are what `m80 run` reads. For
explicit `--install-root` captures, lookup is rooted at
`<install-root>/config.toml` and `<install-root>/profiles/<name>.toml` so
fixture and proof installs can be inspected without touching host state.

The resolver returns one typed state:

- `healthy_active_release`: config selects the installed profile and
  `<install-root>/active` points at the same version directory.
- `missing_active_pointer`: the selected profile is install-shaped but
  `<install-root>/active` is absent.
- `dangling_active_pointer`: `<install-root>/active` points at a missing
  version directory.
- `local_dev_tree`: the selected profile is the built-in `env` profile, so
  artifact paths come from environment/default discovery instead of an installed
  release tree.
- `stale_profile_target`: the selected profile references one version
  directory while `<install-root>/active` points at another.
- `explicit_override`: an operator override such as `M80_DEFAULT_PROFILE`, a
  user config layer, a config drop-in, or a future `--profile` caller override
  selects a profile instead of trusting the installed system config alone.
- `missing_install_metadata`: the active/profile state is coherent, but a
  required installed metadata file is absent.
- `stale_install_metadata`: installed metadata parses, but one of its recorded
  paths, sizes, release tags, or digests no longer matches the active version
  directory.
- `tampered_proof_cache`: the saved public proof-cache manifest or one of its
  referenced proof files no longer matches the recorded digest.
- `invalid_install_metadata`: config/profile parsing failed, the active pointer
  is malformed, or persisted installed-profile paths are not valid install-root
  paths.

Diagnostics are structured by code, field, path, and message. The resolver
rejects active-pointer traversal (`..`), active targets outside
`<install-root>/versions/`, active targets that are not exactly one version
directory, install-owned profile path traversal, and install-owned profile
paths outside the install root. Host prerequisite paths such as Firecracker and
jailer binaries may still point at their documented system locations. Explicit
operator profile overrides are reported as overrides rather than rejected as
broken installed metadata.

## Installed Status Command

`m80 install-status` is the smallest operator-facing wrapper around the
resolver. It is read-only: it does not execute installed binaries, run
preflight, fetch release metadata, repair profiles, or download bundles.

Human output is line-oriented and includes:

- `status`: one resolver state from the finite list above.
- `active_release_tag`, `active_install_dir`, `active_pointer_path`, and
  `active_pointer_target`.
- `selected_config_default_profile` and
  `selected_config_default_profile_source`.
- `selected_profile`, `selected_profile_source`,
  `selected_profile_body_source`, `selected_profile_path`,
  `selected_profile_artifact_dir`, `selected_profile_install_dir`, and
  `selected_profile_release_tag`.
- `bundle_metadata_path`, `host_binaries_manifest_path`,
  `install_provenance_path`, and `proof_cache_manifest_path`, each paired with
  a metadata status. When the active/profile state is too broken to read
  metadata, these fields render as `unavailable`.
- `proof_cache_status`, `proof_cache_path`,
  `proof_cache_manifest_sha256`, `proof_cache_manifest_digest`,
  `proof_cache_manifest_modified_unix_seconds`, `proof_cache_age_seconds`,
  `proof_cache_trust_policy_sha256`, and one indexed
  `proof_cache_material_<n>_*` group per saved public proof artifact. Missing
  active installs and local development profiles report explicit non-cache
  statuses rather than omitting the field.
- `last_attempt_status`, `last_attempt_type`, `last_attempt_target_tag`,
  `last_attempt_failure_stage`, and `last_attempt_repair_command` from
  `<install-root>/last-install-attempt.json`.
- `next_action`, plus `next_action_command` when the state has an executable
  repair or smoke command.

`m80 --json install-status` wraps the same contract in the CLI JSON envelope
with `schema_version: 1`. The stable top-level payload fields are `status`,
`install_root`, `active`, `selected_config`, `selected_profile`, `metadata`,
`last_attempt`, `proof_cache`, `diagnostics`, and `next_action`. `status` uses the resolver
state enum; `active.status` uses `live`, `missing`, `dangling`, or `invalid`;
metadata file statuses use `present`, `missing`, `invalid`, or `stale` when
metadata is available; proof-cache status uses `available`,
`missing_active_install`, `local_dev_install`, `missing_manifest`,
`invalid_manifest`, `stale_manifest`, or `unavailable`; and
`next_action.kind` uses `ready`, `install_release`, `reinstall_release`, or
`remove_override`.

JSON field table:

| Field | Meaning | Evidence use |
| --- | --- | --- |
| `schema_version` | Installed-status payload schema. | Must be `1` for current release evidence. |
| `status` | One finite resolver state. | Primary local install health classification. |
| `install_root` | Root inspected by this status invocation. | Distinguishes default `/opt/m80` from explicit install-root captures. |
| `active.status` | `live`, `missing`, `dangling`, or `invalid`. | Shows whether `<install-root>/active` selected a usable version directory. |
| `active.release_tag` | Release tag parsed from the active version directory. | Pairs status evidence with the release being promoted or repaired. |
| `active.install_dir` | Active version directory path. | Confirms the selected installed tree under `<install-root>/versions/`. |
| `selected_config.default_profile` | Effective profile name selected by config/env/flag layers. | Explains which profile `m80 run` will use by default. |
| `selected_config.default_profile_source` | Config layer that selected the default profile. | Distinguishes installed system config from env/user/drop-in/flag overrides. |
| `selected_config.explicit_override` | Whether the selected default came from an override source. | Prevents treating intentional override captures as installed-default proof. |
| `selected_profile.*` | Selected profile name, source, body source, paths, release tag, and m80 version. | Confirms the profile points at the same installed release as the active pointer. |
| `metadata.bundle_metadata.path/status/sha256` | Bundle metadata path, status, and digest. | Evidence that installed bundle metadata is present and untampered. |
| `metadata.host_binaries_manifest.path/status/sha256` | Host-binaries manifest path, status, and digest. | Evidence for final host-side TCB paths after install. |
| `metadata.install_provenance.path/status/sha256` | Install provenance path, status, and digest. | Evidence for relocation/installer provenance. |
| `metadata.proof_cache_manifest.path/status/sha256` | Proof-cache manifest path, status, and digest. | Evidence that public verification material was preserved. |
| `last_attempt.status` | Whether bounded diagnostic attempt metadata is missing, present, or invalid. | Shows whether the installer left local explanation state after its last command. |
| `last_attempt.attempt_type` | `successful_upgrade`, `verification_failed`, `downgrade_refused`, or `rollback_unsupported`. | Explains the last install-policy outcome without reading logs. |
| `last_attempt.target_tag` | Release tag selected by the attempted install when known. | Names the requested release without preserving source URLs or temp paths. |
| `last_attempt.failure_stage` | Bounded stage label such as `bundle_verification`, `release_transition`, or `unsupported_operation`. | Points support at the failed phase without leaking staging paths or secrets. |
| `last_attempt.repair_command` | Copyable next command when the outcome has one. | Lets status repeat the command after the original install output is gone. |
| `proof_cache.status` | Local cached-proof state. | Distinguishes available proof material from missing active installs, local dev profiles, missing manifests, invalid manifests, and stale material. |
| `proof_cache.cache_dir` and `proof_cache.manifest_path` | Versioned proof-cache directory and manifest path. | Shows the exact installed tree used for offline evidence. |
| `proof_cache.manifest_sha256` and `proof_cache.manifest_digest` | Full manifest file digest and canonical payload digest. | Confirms both the saved JSON file and payload contract. |
| `proof_cache.manifest_modified_unix_seconds` and `proof_cache.cache_age_seconds` | Local source timestamp and age for the saved manifest. | Shows when the offline cache material was last written locally. |
| `proof_cache.materials[]` | Saved public material role, path, sha256, size, subject, and source timestamp. | Evidence of every public artifact preserved after install-time verification. |
| `proof_cache.trust_policy.path/identity/sha256` | Trust policy saved with the verification material. | Captures the identity policy that bounded install-time verification. |
| `proof_cache.verifier_versions.*` | m80, native release-attestation verifier, release-integrity schema, and asset-index schema versions. | Explains which local verifier versions produced the saved evidence. |
| `proof_cache.diagnostics[]` | Proof-cache stale/missing/invalid diagnostics copied from the local status reader. | Distinguishes local cache tampering from public latest freshness checks. |
| `proof_cache.repair_command` | Pinned reinstall command for the affected release tag when the cache is missing, invalid, or stale. | Gives support captures a deterministic repair path without selecting mutable latest. |
| `diagnostics[]` | Resolver diagnostics with code, field, path, message, `repair_command`, and `rollback_command`. | Machine-readable failure details plus copyable operator repair commands for support captures. |
| `mismatches[]` | Expected/observed mismatch records for stale defaults and overrides. | Shows exactly which tag/path/source differs from the installed default. |
| `next_action.kind` | `ready`, `install_release`, `reinstall_release`, or `remove_override`. | Stable automation hint for local repair UX. |
| `next_action.command` | Exact command when a safe command exists. | Copyable repair or smoke command; absent for override removal. |

Status output also includes `mismatches`, a structured list for cases where the
selected local state is intentionally or accidentally not the installed default.
Each mismatch has a finite `code`, a message, and any applicable
`expected_path`, `observed_path`, `expected_tag`, `observed_tag`,
`expected_source`, `observed_source`, `expected_value`, and `observed_value`.
The status command uses this list to make these cases distinct:

- `stale_profile_target`: the default installed profile points at one release
  directory while `<install-root>/active` points at another. The expected fields
  name the active pointer release tag and directory; the observed fields name
  the selected profile release tag and directory.
- `explicit_profile_override`: an environment variable, flag, user config, or
  drop-in selected a profile instead of the installed system config. This is
  intentional override state, not a stale default profile.
- `selected_profile_unavailable`: effective config names a default profile, but
  that profile cannot be resolved.
- `install_root_override`: status is inspecting a non-default install root. By
  default, with no override, `m80 install-status` inspects `/opt/m80`, reads the
  installed system config, and expects that config's default profile to point at
  the same version directory as `/opt/m80/active`.

Quickstart troubleshooting starts with `m80 install-status` to decide whether
the local release bundle/profile pair is coherent. Host substrate problems stay
in `m80 preflight`; update freshness stays in the release freshness monitor.

`m80 update --check` reuses this installed-state reader before it compares the
active install with latest release metadata. Its JSON output keeps the local
install facts separate from freshness classification: `active_tag`,
`latest_stable_tag`, `active_kind`, `freshness_state`, `latest_status_source`,
`latest_status_origin`, `latest_status_cache_state`,
`latest_status_fetched_at`, `latest_status_max_age_seconds`,
`latest_status_offline_reason`, `safety_state`, `safety_floor.status`,
`proof_cache_status`, `proof_cache_age_seconds`, `apply_command`,
`reinstall_command`, and `retry_command`. `active_kind` is one of
`stable_release`, `prerelease`, `ineligible`, `local_dev`, `missing_active`, or
`stale_active_metadata`; `freshness_state` is one of `current`, `outdated`,
`unknown_offline`, `stale_latest_metadata`, `prerelease_active`,
`ineligible_active`, `local_dev_install`, `install_unhealthy`, `yanked`, or
`unsafe`. The fields are documented in
[`update-check.md`](update-check.md); this page names them because stale local
install state is the input that determines whether freshness comparison is
eligible at all.

Repair examples:

```text
status=healthy_active_release
next_action=installed release is ready
next_action_command=m80 run -- echo hello
```

```text
status=missing_active_pointer
diagnostic_0_code=missing_active_pointer
diagnostic_0_repair_command=curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
diagnostic_0_rollback_command=sudo ln -sfnT -- '/opt/m80/versions/v1.2.3' '/opt/m80/active'
next_action=install a release to create the active install pointer
next_action_command=curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```

```text
status=stale_profile_target
mismatch_0_code=stale_profile_target
mismatch_0_expected_tag=v1.2.4
mismatch_0_observed_tag=v1.2.3
diagnostic_0_code=profile_targets_inactive_version
diagnostic_0_repair_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh
diagnostic_0_rollback_command=sudo ln -sfnT -- '/opt/m80/versions/v1.2.3' '/opt/m80/active'
next_action=reinstall the selected release to refresh the installed bundle
next_action_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh
```

```text
status=explicit_override
mismatch_0_code=explicit_profile_override
next_action=remove the profile override to inspect the installed default release
```

## Installed Metadata Reader

The metadata reader for `m80-o3uh9.16.7.3` is the status-facing view of the
active version directory. It parses these files without running binaries,
preflight, network fetches, or installer repair:

- `<version-dir>/bundle.json`
- `<version-dir>/artifacts/install-provenance.json`
- `<version-dir>/artifacts/host-binaries.manifest.json`
- `<version-dir>/artifacts/release-proof-cache/manifest.json`

The reader treats unknown JSON fields, malformed digests, absolute or escaping
bundle file paths, malformed proof-cache file names, symlinked installed references,
and missing referenced files as unhealthy installed state. Metadata
files are opened with `O_NOFOLLOW`, referenced bundle/proof/provenance files
are hashed from an opened file descriptor instead of read fully into memory, and
relative installed references must not traverse symlink components inside the
active version directory. Bundle metadata and proof-cache DTOs use
`deny_unknown_fields`; host-binaries and install-provenance parsing reuses the
shared manifest readers from `m80-image-manifest`. A missing metadata file
reports `missing_install_metadata`; a stale bundle/provenance reference reports
`stale_install_metadata`; a stale proof-cache reference reports
`tampered_proof_cache`.

## Proof Cache Contract

Official release installs preserve the public proof material under
`<install-root>/versions/<release_tag>/artifacts/release-proof-cache/`:

```text
<install-root>/versions/<release_tag>/artifacts/release-proof-cache/
```

The cache manifest is
`<install-root>/versions/<release_tag>/artifacts/release-proof-cache/manifest.json`:

```text
<install-root>/versions/<release_tag>/artifacts/release-proof-cache/manifest.json
```

`manifest.json` has `schema_version: 1`, a top-level `manifest_digest`, and a
`payload` object. `manifest_digest` is the sha256 of the canonical JSON payload,
not a digest of the enclosing manifest object. The payload records what the
installer verified; it is evidence for offline status and repair diagnostics,
not a second trust root.

The payload fields are:

- `release_tag`, `repository`, and `target`;
- `integrity_predicate`: path, sha256, and size for
  `m80-release-integrity.json`;
- `attestation_bundle`: path, sha256, and size for
  `m80-release-integrity.attestation.jsonl`;
- `attestation_metadata`: path, sha256, size, signer identity, issuer, keyset,
  and predicate sha256 for `m80-release-attestation.json`;
- `asset_index`: path, sha256, and size for `m80-release-assets.json`;
- `public_sha256s`: path, sha256, and size for public `SHA256SUMS`;
- `checksum_sidecars`: every public checksum sidecar path, sha256, and subject;
- `trust_policy`: policy path, trust identity, and policy sha256;
- `verifier_versions`: m80 version, attestation verifier, release-integrity schema
  version, and asset-index schema version.

The parser is typed and uses `deny_unknown_fields`. Missing required fields,
unknown fields, empty path/identity strings, zero sizes, unsupported schema
versions, malformed sha256 fields, and mismatched `manifest_digest` all fail
closed before any status surface trusts the cached proof.

The proof-cache reporter used by `m80 install-status` and `m80 --json preflight`
is offline-only. It reads the installed cache manifest and saved public material
from the selected version directory and exposes saved paths and hashes. It
never fetches release metadata and never reruns the remote trust decision. The
fields are evidence of what the installer verified at install time. Freshness
against the current public latest release belongs to the freshness lane, not
this cache reporter.

The current public proof artifacts for this contract are
[`public-install-proof-cache-v0.2.11.json`](public-install-proof-cache-v0.2.11.json)
and
[`public-install-status-v0.2.11.json`](public-install-status-v0.2.11.json).
They record the unauthenticated latest install command, the resolved release
tag, the proof-cache manifest digest/material count, the full installed-status
JSON output, and the `m80 run -- echo hello` smoke run used to prove the
installed bundle wraps a real process.

When the local proof cache is missing, malformed, stale, mode-changed, or has a
referenced material digest mismatch, status reports `proof_cache.status` as a
local cache problem and includes the proof-cache diagnostic plus a pinned
release reinstall command when the affected release tag is URL-safe. This does
not decide whether the release is current; it only blocks reuse of local saved
trust material until repair or reinstall.

## Last Attempt Metadata

The installer writes `<install-root>/last-install-attempt.json` as bounded
diagnostic state after successful upgrades, failed verification, refused
downgrades, and unsupported rollback requests. This file is intentionally
outside the versioned bundle, generated host-binaries manifest, install
provenance, and proof cache. Trust decisions never read it; it only lets
`m80 install-status` explain the last command after stdout/stderr have been
lost.

The schema is small by design:

```json
{
  "schema_version": 1,
  "attempt_type": "downgrade_refused",
  "target_tag": "v1.2.2",
  "failure_stage": "release_transition",
  "repair_command": "curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.3/install.sh | sudo sh"
}
```

The writer records only finite attempt types, release tags, finite failure
stage labels, and repair commands. It does not persist raw bundle URLs,
temporary extraction paths, request IDs, environment variables, or verifier
stderr. If writing this metadata fails, the installer still returns the primary
install failure or success; attempt metadata write errors are diagnostic loss,
not install-state authority.

## Repair And Rollback Diagnostics

Status diagnostics are read-only. They never create directories, restore
profiles, rewrite default config, or flip the active pointer by themselves.
For install-root states that have a deterministic repair, each diagnostic
prints `diagnostic_<n>_repair_command` in human output and
`diagnostics[n].repair_command` in JSON.

Missing active pointers and local development installs use the stable latest
install command because there is no active tag to pin. Dangling active
pointers, stale default profiles, missing installed metadata, stale installed
metadata, invalid installed metadata, and proof-cache tampering use a pinned
install command for the active release tag when that tag is URL-safe, falling
back to the selected profile tag or latest only when there is no safe tag.

When the selected installed profile already points at another versioned install
directory, status also prints a manual rollback command:

```text
sudo ln -sfnT -- '<install-root>/versions/<previous-tag>' '<install-root>/active'
```

Only the active symlink is operator-changeable for this manual rollback path.
The version directory, `bundle.json`, generated profile fields,
`artifacts/install-provenance.json`, `artifacts/host-binaries.manifest.json`,
and `artifacts/release-proof-cache/manifest.json` are installer-generated
state. Do not hand-edit those files; reinstall the affected release instead.
There is no `m80 rollback` command in this tranche; downgrade-by-install stays
refused by default, and explicit rollback is limited to this status-rendered
manual active-pointer move for an already-installed version.

## Transaction Ordering

For official release installs, the installer writes the proof cache inside the
staged version directory after extraction and installed metadata rewriting, but
before the staged tree is renamed to `<install-root>/versions/<release_tag>`,
before default profile/config writes, and before `<install-root>/active` is
renamed. The writer copies the verified public material, writes the trust
policy used by the verifier, computes the manifest digest, reads the manifest
back through the typed parser, and checks the cache directory/file modes before
the install can proceed to version-directory publication, host-binaries manifest
generation, and profile publishing.

If proof-cache file creation, manifest rehashing, or mode checking fails, the
staged tree is not renamed into the version directory and is not active. Any
previous active pointer, default profile, and config file remain selected. A
pre-existing non-directory proof-cache target is rejected without rewriting the
colliding path.

If the requested official release tag is already installed and active, the
installer compares the newly verified bundle and public proof material with the
saved installed tree before writing any active state. The idempotent no-op path
requires the raw active-pointer target, installed release bytes, installed
`bundle.json`, installed `SHA256SUMS`, install provenance, default profile,
install-owned selector fields in `config.toml`, host-binaries manifest paths
and installed m80 binary hashes, and proof cache to verify. Identical public
proof material is reported as `state=already_installed` and
`reinstall_status=idempotent_same_material`: the active pointer, default
profile, config, and installed release bytes remain unchanged, `files_copied=0`,
and both the existing and newly verified proof-cache manifest digests are
printed.
`verifier_versions` drift alone is manifest provenance, not changed public
trust material, so it is reported as idempotent while still showing old/new
manifest digests. If the same tag now resolves to changed public trust material,
the reinstall fails closed with `proof-cache.reinstall`,
`existing_manifest_digest`, `verified_manifest_digest`, the changed proof
fields, `version_dir`,
`explicit_repair=review_changed_public_material_then_remove_version_dir_and_reinstall`,
and an exact `repair_command`.
m80 does not silently replace saved trust material for a same-version reinstall.
If the version directory exists but `<install-root>/active` points somewhere
else, m80 refuses the reinstall as `install.active_pointer` instead of guessing
which installed tree owns the same tag. Changed installed bytes, generated
profile fields, installed config selector fields, or host-binaries metadata
fail as `install.reinstall` with the same repair command and do not overwrite
by default.

## Tests

- `complete_manifest_parses_and_validates_digest`
- `missing_required_field_fails_closed`
- `unknown_field_fails_closed`
- `malformed_material_digest_fails_closed`
- `malformed_manifest_digest_fails_closed`
- `write_verified_release_proof_cache_copies_manifest_and_mode_checks_material`
- `write_verified_release_proof_cache_rejects_existing_cache_target_file`
- `proof_cache_write_failure_leaves_previous_active_profile_and_config_selected`
- `proof_cache_manifest_digest_failure_leaves_previous_active_profile_and_config_selected`
- `proof_cache_mode_failure_leaves_previous_active_profile_and_config_selected`
- `same_version_reinstall_with_identical_proof_material_is_idempotent`
- `same_version_reinstall_with_stale_installed_byte_refuses_explicit_repair`
- `same_version_reinstall_with_missing_installed_byte_refuses_explicit_repair`
- `same_version_reinstall_with_missing_proof_cache_manifest_refuses_explicit_repair`
- `same_version_reinstall_with_stale_host_manifest_refuses_explicit_repair`
- `same_version_reinstall_with_stale_profile_kernel_kind_refuses_explicit_repair`
- `same_version_reinstall_with_missing_default_profile_refuses_explicit_repair`
- `same_version_reinstall_with_missing_installed_config_refuses_explicit_repair`
- `same_version_reinstall_with_changed_verifier_versions_is_idempotent`
- `same_version_reinstall_with_changed_predicate_refuses_silent_replacement`
- `same_version_reinstall_with_changed_public_sha256s_refuses_silent_replacement`
- `same_version_reinstall_with_changed_trust_policy_identity_refuses_silent_replacement`
- `same_version_reinstall_change_error_names_explicit_repair_version_dir`
- `same_version_reinstall_refuses_when_existing_version_is_not_active`
- `same_version_reinstall_refuses_when_active_pointer_is_missing`
- `human_layout_summary_includes_reinstall_diagnostics`
- `install_state_doc_names_proof_cache_manifest_contract`
- `status_matrix_healthy_active_release`
- `status_matrix_missing_active_pointer`
- `status_matrix_dangling_active_pointer`
- `status_matrix_stale_profile_target`
- `status_matrix_explicit_override`
- `status_matrix_explicit_override_flag_source`
- `status_matrix_local_dev_tree`
- `status_matrix_tampered_proof_cache`
- `resolver_reports_tampered_proof_cache`
- `resolver_reports_tampered_proof_cache_for_missing_reference`
- `resolver_rejects_proof_cache_symlink_even_when_digest_matches`
- `resolver_reports_tampered_proof_cache_for_manifest_digest_mismatch`
- `resolver_reports_stale_metadata_for_missing_kernel_reference`
- `resolver_reports_tampered_proof_cache_for_changed_file_mode`
- `resolver_reports_tampered_proof_cache_for_changed_manifest_mode`
- `resolver_reports_tampered_proof_cache_for_changed_cache_dir_mode`
- `resolver_reports_proof_cache_materials_for_offline_status`
- `json_output_reports_active_install_paths`
- `human_output_reports_active_install_paths`
- `preflight_json_report_includes_selected_profile_context`
- `preflight_json_report_reads_offline_proof_cache_material`
- `preflight_json_report_reports_tampered_proof_cache_repair_command`
