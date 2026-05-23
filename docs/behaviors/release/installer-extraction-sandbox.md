# Installer Extraction Sandbox

Behavior bead: `m80-o3uh9.3.6`.

`m80 install` treats release bundle contents as untrusted until the bundle has
passed entry-set validation, extraction-shape validation, metadata validation,
payload hashing, and `SHA256SUMS` verification.

## Staging Rule

The installer never extracts a bundle into the active install root. After the
bundle URL and release material checks pass, bytes are staged under:

```text
<install-root>/.staging/layout-<pid>/bundle.tar.gz
<install-root>/.staging/layout-<pid>/bundle/
```

The version directory under `<install-root>/versions/<release_tag>` is created
only after the staged extraction has been verified and rewritten. The active
pointer flips last. A malformed bundle therefore may create a transient private
staging directory, but it must not publish a version directory or change
`<install-root>/active`.

## Fail-Closed Rules

Before extraction, the tar listing is normalized and checked against the exact
bundle contract. Absolute paths, parent components, backslash separators,
duplicates, missing required files, and unexpected paths fail before extraction.

After extraction, the staged tree is walked with symlink metadata. Every
required payload must be a regular file at the exact required path. Symlinks,
FIFOs, device nodes, and other non-regular files fail. Hardlinked payload files
fail because release payloads must not share inode identity. Directories are
allowed only at `bin/` and `artifacts/`; a directory where a required file is
expected fails as an unexpected extracted directory.

The staged payload modes are part of the bundle contract:

```text
bin/m80                  0755
bin/m80-jailer-harden    0755
bin/m80-net-helper       0755
install.sh               0755
all other required files 0644
```

Mode mismatches fail before metadata rewrite, final mode normalization, profile
writes, version publication, or active pointer changes.

## Tests

- `install_bundle_layout_copies_verified_bundle_into_version_dir`
- `install_bundle_layout_duplicate_bundle_path_fails_before_activation`
- `install_bundle_layout_symlink_payload_fails_before_activation`
- `install_bundle_layout_hardlink_payload_fails_before_activation`
- `install_bundle_layout_directory_payload_fails_before_activation`
- `install_bundle_layout_device_like_payload_fails_before_activation`
- `install_bundle_layout_bad_payload_mode_fails_before_activation`
- `normalize_tar_entry_rejects_escaping_directories_before_extract`
