# Installed Layout

Behavior beads: `m80-o3uh9.3.3`, `m80-o3uh9.3.7`.

`m80 install --bundle-url <URL> --install-root <PATH>` stages one release
bundle, verifies it, and copies it into one versioned directory. The layout
transaction accepts explicit local `file://...` bundles for fixtures and
`https://github.com/moradology/m80/releases/download/...` release bundle URLs.
Local HTTP is accepted only for test fixtures.

The layout leaf still does not resolve release tags, install host
prerequisites, write profile state, or switch the active install pointer.

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

`<install-root>/active` is unchanged by this leaf. Runtime profile generation
is also unchanged. Later installer leaves own host-prerequisite validation,
profile writing, active-pointer finalization, and smoke-probe behavior.

## Failure Contract

The installer validates the tar entry set before extraction. Duplicate paths,
unexpected paths, escaping paths, missing required files, unsupported bundle
metadata, `SHA256SUMS` mismatch, and non-regular extracted files fail closed.

The copy uses `<install-root>/.staging/layout-<pid>` as its transaction
directory. Remote bundle downloads first land as
`<install-root>/.staging/layout-<pid>/bundle.tar.gz`, and the adjacent
`<URL>.sha256` is downloaded and checked before extraction. The final effective
download URL must stay on the release host/CDN allowlist, or on the same local
test fixture authority. Indexed size and digest metadata handoff is tracked by
`m80-o3uh9.3.10`; this leaf's explicit remote URL path is checksum-sidecar
verified.

Failures never write `<install-root>/active` and never write profile state.
Failed downloads, checksum mismatches, unsupported redirects, and truncated
downloads delete staged bundle/checksum partials. Failures after extraction may
leave the staging directory behind so the operator can inspect the partial
extraction.

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
- `install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout`
- `installed_layout_doc_names_directory_contract_and_tests`
