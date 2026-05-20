# Installed Layout

Behavior bead: `m80-o3uh9.3.3`.

`m80 install --bundle-url file://... --install-root <PATH>` copies one verified
release bundle into one versioned directory. The layout leaf is intentionally
only the file transaction: it does not resolve release tags, download network
bundles, install host prerequisites, write profile state, or switch the active
install pointer.

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

The copy uses `<install-root>/.staging/layout-<pid>/bundle` as its transaction
directory. Failures never write `<install-root>/active` and never write profile
state. Failures after staging may leave that staging directory behind so the
operator can inspect the partial extraction.

`--dry-run` remains a pure plan render. It does not read the bundle and does not
create `<install-root>`.

## Tests

- `install_bundle_layout_copies_verified_bundle_into_version_dir`
- `install_bundle_layout_missing_required_bundle_file_fails_before_activation`
- `install_bundle_layout_duplicate_bundle_path_fails_before_activation`
- `install_bundle_layout_permission_failure_leaves_active_state_untouched`
- `install_bundle_layout_dry_run_never_reads_or_writes_bundle_layout`
- `installed_layout_doc_names_directory_contract_and_tests`
