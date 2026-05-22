# Install Path Canonicalization

Bead: `m80-o3uh9.16.3`

`m80 install` writes persistent install state only with absolute paths. A
relative `--install-root` is normalized against the command working directory
before planning, dry-run rendering, bundle extraction, profile writing, host
manifest generation, and active-pointer selection.

The installer fails closed before staging if the install root is an existing
symlink or if any existing ancestor in the install-root path is a symlink. It
also refuses a pre-existing active pointer whose target is relative, missing,
contains `..`, escapes `<install-root>/versions`, or points below a nested path
instead of one concrete version directory.

Installed state uses these path rules:

- `<install-root>/versions/<tag>` is the only active-pointer target shape.
- `<install-root>/active` points to an absolute version directory.
- install-root overrides keep profile, config, and run-root paths under the
  absolute install root.
- the default profile records absolute artifact, helper, manifest, provenance,
  host-binaries manifest, and run-root paths.
- `install-status` classifies relative active-pointer targets as invalid rather
  than resolving them implicitly.

Coverage:

- `crates/m80-cli/tests/release/installer_layout/path_canonicalization.rs`
  covers relative install-root normalization, symlinked install-root refusal,
  symlink-ancestor refusal, relative active-pointer refusal, and dangling
  active-pointer refusal before a new version directory is published.
