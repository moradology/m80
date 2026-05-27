# Installed Layout

Behavior beads: `m80-o3uh9.3.3`, `m80-o3uh9.3.4`, `m80-o3uh9.3.6`,
`m80-o3uh9.3.7`, `m80-o3uh9.16.1`.

`m80 install --release-tag <TAG> --install-root <PATH>` fetches the pinned
release asset index, verifies it through signed release-integrity material,
selects the matching Linux x86_64 minimal bundle, stages that release bundle,
verifies the extracted layout, copies it into one versioned directory, writes
install metadata/profile state, and flips the active pointer last.
`--bootstrap-tag <TAG>` uses the same indexed source selection after the
bootstrapper resolves "latest" to a concrete tag. The installer also accepts
explicit local `file://...` bundles for fixtures and concrete stable-tag
`https://github.com/moradology/m80/releases/download/<tag>/m80-<target>.tar.gz`
release bundle URLs. Direct official release URLs are not a weaker trust mode:
the installer verifies the same release-integrity material and GitHub
attestation policy as the public installer before staging or tar listing.
Mutable latest artifact URLs, raw branch URLs, foreign
repositories, path-traversal asset paths, non-bundle release assets, and
non-HTTPS GitHub release URLs fail before network access or install-root
mutation. Local HTTP is accepted only for test fixtures.

## Directory Contract

A successful install creates `<install-root>/versions/<release_tag>` with these
bundle payloads:

```text
bin/m80
bin/m80-jailer-harden
bin/m80-net-helper
artifacts/vmlinux
artifacts/output.ext4
artifacts/output.ext4.manifest.json
artifacts/output.ext4.build-receipt.json
artifacts/m80-guestd
artifacts/install-provenance.json
artifacts/host-binaries.manifest.json
install.sh
bundle.json
SHA256SUMS
```

The copied guest manifest and build receipt are rewritten from release-local
paths to the final versioned `artifacts/` paths. The rewrite is not silent:
`artifacts/install-provenance.json` records the source hash, installed hash,
release tag, and `install_path_rewrite` transform for both rewritten JSON
artifacts. The installed `bundle.json` and bundled `SHA256SUMS` are rewritten
after those path rewrites so the version directory is self-verifying on disk.
For official release installs, the release integrity predicate, attestation
material, asset index, and trust policy are preserved in
`artifacts/release-proof-cache/`.

The installer also publishes a flat hardlink projection:

```text
<install-root>/bin/m80
<install-root>/bin/m80-net-helper
<install-root>/artifacts/vmlinux
<install-root>/artifacts/output.ext4
<install-root>/artifacts/output.ext4.manifest.json
<install-root>/artifacts/output.ext4.build-receipt.json
<install-root>/artifacts/m80-guestd
<install-root>/artifacts/install-provenance.json
<install-root>/artifacts/host-binaries.manifest.json
```

On wrapper-fallback hosts the flat projection also contains
`<install-root>/bin/m80-jailer-harden`. On systemd-selected hosts the versioned
directory still keeps the bundled wrapper for rollback/multi-version records,
but the flat client-facing projection omits the wrapper and records the
conditional absence in `host-binaries.manifest.json`.

The flat binaries and payload artifacts are hardlinks to the selected versioned
directory, not symlinks. Flat guest metadata is regenerated with flat absolute
paths and published before the flat `host-binaries.manifest.json`; the host
manifest is written last so clients see either the previous complete projection
or the new complete projection. `release-proof-cache/` is hardlink-copied when
official release proof material is present. The versioned metadata remains the
install record used for rollback and multi-version coexistence.

The canonical executable entrypoint is `<install-root>/bin/m80` (`bin/m80`
inside the flat projection); guest metadata is rooted at
`<install-root>/artifacts/output.ext4.manifest.json`
(`artifacts/output.ext4.manifest.json`).

The installer also publishes `<bin-dir>/m80` as a symlink to the installed
release binary and verifies that `command -v m80` resolves to that path before
flipping `<install-root>/active`. The default bin directory is `/usr/local/bin`
for `/opt/m80` installs and `<install-root>/bin` for explicit fixture/proof
roots; `--bin-dir <PATH>` overrides it. When the command handoff path is the
flat `<install-root>/bin/m80`, the installer verifies the hardlink instead of
replacing it with a symlink. The detailed command handoff contract is captured
in [`installer-path-handoff.md`](installer-path-handoff.md).

For the default install root `/opt/m80`, `/etc/m80/profiles/default.toml`
points at the flat artifact paths, generated flat host-binaries manifest, and
flat m80 helper binaries.
`/etc/m80/config.toml` selects that profile as `default`. For explicit
`--install-root` fixture/proof installs, the same selector files are rooted at
`<install-root>/profiles/default.toml` and `<install-root>/config.toml`.
`<install-root>/active` is an absolute symlink to the selected version
directory and changes only after the finalization transaction succeeds. The
client-facing host manifest path is
`<install-root>/artifacts/host-binaries.manifest.json`; the version directory
keeps its own `artifacts/host-binaries.manifest.json` as the versioned install
record.

## Failure Contract

The installer validates the tar entry set before extraction. Duplicate paths,
unexpected paths, escaping paths, missing required files, unsupported bundle
metadata, `SHA256SUMS` mismatch, non-regular extracted files, hardlinked
payload files, unexpected payload directories, and wrong payload modes fail
closed. The detailed extraction sandbox contract is captured in
[`installer-extraction-sandbox.md`](installer-extraction-sandbox.md).

The copy uses `<install-root>/.staging/layout-<pid>` as its transaction
directory. Abandoned `layout-*` staging directories are deleted before a new
install starts, and the active staging directory is removed on success or
failure. Remote bundle downloads first land as
`<install-root>/.staging/layout-<pid>/bundle.tar.gz`. The final effective
download URL must stay on the release host/CDN allowlist, or on the same local
test fixture authority. For indexed release-tag and bootstrap installs, the
selected asset-index row's bundle URL, sha256, `size_bytes`, metadata identity,
release tag, target tuple, image kind, and m80 version are carried into
release-material verification before extraction. The bundle is not accepted
unless its signed predicate digest, computed sha256, and downloaded byte length
match the selected index material. Explicit local `file://` fixture bundles are
the only install source that may omit indexed size and digest material.

Verification failures never write `<install-root>/active`. Failures after a
previous install leave the previous active symlink selected. Profile-write,
host-binaries manifest, and injected interruption failures can leave an
unselected version directory behind for inspection, but the active pointer is
not changed. Failed downloads, checksum mismatches, unsupported redirects, and
truncated downloads delete staged bundle/checksum partials.
Direct official release-material failures happen earlier than staging: missing
metadata, wrong repository, wrong tag, stale integrity material or asset index rows,
bad attestation, bad predicate, tampered bundle bytes, and release-material
network failures leave the install root byte-for-byte unchanged and print the
failed `material_class` plus a safe retry command.

`--dry-run` remains a pure plan render. It does not read the bundle and does not
create `<install-root>`.

## Tests

- `install_bundle_layout_copies_verified_bundle_into_version_dir`
- `install_bundle_layout_downloads_http_bundle_into_version_dir`
- `install_bundle_layout_rejects_remote_bundle_checksum_mismatch_before_extract`
- `install_bundle_layout_rejects_remote_404_before_extract`
- `install_bundle_layout_deletes_truncated_download_partial`
- `install_bundle_layout_rejects_redirect_to_different_fixture_host`
- `install_bundle_layout_rejects_checksum_redirect_to_different_fixture_host`
- `install_bundle_layout_missing_required_bundle_file_fails_before_activation`
- `install_bundle_layout_duplicate_bundle_path_fails_before_activation`
- `install_bundle_layout_fails_when_older_m80_shadows_installed_path`
- `install_bundle_layout_explicit_bin_dir_override_controls_handoff_path`
- `install_bundle_layout_symlink_payload_fails_before_activation`
- `install_bundle_layout_hardlink_payload_fails_before_activation`
- `install_bundle_layout_directory_payload_fails_before_activation`
- `install_bundle_layout_device_like_payload_fails_before_activation`
- `install_bundle_layout_bad_payload_mode_fails_before_activation`
- `install_bundle_layout_permission_failure_leaves_active_state_untouched`
- `install_bundle_layout_manifest_failure_leaves_previous_active_selected`
- `install_bundle_layout_profile_failure_leaves_previous_active_and_profile`
- `install_bundle_layout_injected_interruption_leaves_previous_active_selected`
- `install_bundle_layout_cleans_abandoned_staging_dirs`
- `install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout`
- `missing_integrity_predicate_aborts_before_install_root_mutation`
- `missing_asset_index_aborts_before_install_root_mutation`
- `integrity_predicate_digest_mismatch_aborts_before_install_root_mutation`
- `tampered_bundle_aborts_before_install_root_mutation`
- `installed_layout_doc_names_directory_contract_and_tests`
