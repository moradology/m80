# Installed Layout

Behavior beads: `m80-o3uh9.3.3`, `m80-o3uh9.3.7`, `m80-o3uh9.16.1`.

`m80 install --release-tag <TAG> --install-root <PATH>` fetches the pinned
release asset index, verifies its checksum sidecar, selects the matching Linux
x86_64 minimal bundle, stages that release bundle through the existing
checksum-sidecar downloader, verifies the extracted layout, copies it into one
versioned directory, writes install metadata/profile state, and flips the active
pointer last. `--bootstrap-tag <TAG>` uses the same indexed source selection
after the bootstrapper resolves "latest" to a concrete tag. The installer also
accepts explicit local `file://...` bundles for fixtures and
`https://github.com/moradology/m80/releases/download/...` release bundle URLs.
Local HTTP is accepted only for test fixtures.

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
paths to the final installed `artifacts/` paths. The rewrite is not silent:
`artifacts/install-provenance.json` records the source hash, installed hash,
release tag, and `install_path_rewrite` transform for both rewritten JSON
artifacts.

The executable entrypoint is `bin/m80`; guest metadata is rooted at
`artifacts/output.ext4.manifest.json`.

`<install-root>/profiles/default.toml` points at the installed artifact paths,
generated host-binaries manifest, and installed m80 helper binaries under the
version directory. `<install-root>/config.toml` selects that profile as
`default`. `<install-root>/active` is an absolute symlink to the selected
version directory and changes only after the finalization transaction succeeds.
The host manifest path is `artifacts/host-binaries.manifest.json`.

## Failure Contract

The installer validates the tar entry set before extraction. Duplicate paths,
unexpected paths, escaping paths, missing required files, unsupported bundle
metadata, `SHA256SUMS` mismatch, and non-regular extracted files fail closed.

The copy uses `<install-root>/.staging/layout-<pid>` as its transaction
directory. Abandoned `layout-*` staging directories are deleted before a new
install starts, and the active staging directory is removed on success or
failure. Remote bundle downloads first land as
`<install-root>/.staging/layout-<pid>/bundle.tar.gz`, and the adjacent
`<URL>.sha256` is downloaded and checked before extraction. The final effective
download URL must stay on the release host/CDN allowlist, or on the same local
test fixture authority. Indexed size and digest metadata handoff is tracked by
`m80-o3uh9.3.10`; this leaf's explicit remote URL path is checksum-sidecar
verified.

Verification failures never write `<install-root>/active`. Failures after a
previous install leave the previous active symlink selected. Profile-write,
host-binaries manifest, and injected interruption failures can leave an
unselected version directory behind for inspection, but the active pointer is
not changed. Failed downloads, checksum mismatches, unsupported redirects, and
truncated downloads delete staged bundle/checksum partials.

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
- `install_bundle_layout_permission_failure_leaves_active_state_untouched`
- `install_bundle_layout_manifest_failure_leaves_previous_active_selected`
- `install_bundle_layout_profile_failure_leaves_previous_active_and_profile`
- `install_bundle_layout_injected_interruption_leaves_previous_active_selected`
- `install_bundle_layout_cleans_abandoned_staging_dirs`
- `install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout`
- `installed_layout_doc_names_directory_contract_and_tests`
