# Release Asset Index

The release asset index is the machine-readable contract that lets
quickstart/bootstrapper code select a bundle without inferring semantics from a
filename. The installer/bootstrapper source path reads the verified index
before bundle download, selects the default bundle by release tag, OS,
architecture, and image kind. The current flat-artifact quickstart path still
requires an explicit artifact URL until it moves fully onto release bundles. An
explicit bundle URL override bypasses index selection.

The published index is `m80-release-assets.json`. Its checksum sidecar is
`m80-release-assets.json.sha256`, and the public `SHA256SUMS` covers the index
alongside the bundle tarball, `install.sh`, the metadata sidecar, and
`m80-bootstrap-selector.tsv` and `m80-release-build.json`.
Installer/bootstrapper code follows this order: fetch the pinned index and its
sidecar for the same concrete release tag, verify the index sha256, and only
then parse JSON or select a host tuple. `file://` fixture indexes use the same
checksum-sidecar verifier as remote release indexes.

The index uses `schema_version: 1` and has one top-level `release_tag`. Every
bundle row records:

- `name` and `url`;
- `sha256` and `size_bytes` for the bundle tarball;
- `metadata_name` and `metadata_sha256` for the byte-identical bundle metadata
  sidecar;
- `checksum_name`, `signature_name`, and `attestation_name` proof references;
- `target`, `os`, and `arch`;
- `image_kind`;
- `release_tag` and `m80_version`;
- `guest_protocol_version`;
- `manifest_schema_version`;
- `expected_firecracker_version`.

The default Linux quickstart row is the single asset with `os: "linux"`,
`arch: "x86_64"`, and `image_kind: "minimal"` whose `release_tag` and
`m80_version` match the running release binary. Duplicate defaults fail closed;
duplicate default rows are invalid for selection.
Missing defaults fail closed. Rows for the wrong architecture, wrong image
kind, wrong tag, or wrong m80 version fail with typed diagnostics that name the
requested tuple and available alternatives.
Diagnostics name the requested `os`, `arch`, `image_kind`, release tag, and
binary version when those fields are relevant. They also name available tuples
as `<os>/<arch>/<image_kind>@<m80_version>` and include either `--bundle-url`
for an explicit compatible bundle or a pinned release `install.sh` URL when a
matching tagged release is visible in the index.

Release dev builds do not select release assets from the index. A dev build
needs an explicit local bundle URL or artifact override so it cannot silently
consume the mutable public latest release. Explicit bundle URLs bypass index
selection but do not bypass later bundle metadata and integrity verification.

The parser rejects unknown fields, unsupported schemas, empty required fields,
non-SHA-256 digest strings, zero size/schema/protocol numbers, and rows whose
`target` does not match `os` plus `arch`. This keeps the index strict enough
for future OS, architecture, or image-kind expansion without changing the
README quickstart command.

Fetch diagnostics name the index URL or local path, checksum sidecar URL or
path, expected sha256, observed sha256, release tag, requested OS/arch, and
image kind. Missing index bytes, missing sidecars, checksum mismatch, invalid
JSON after a valid checksum, stale schema, and index `release_tag` drift all
fail before bundle download, extraction, tuple selection, or active install
state writes.

The release also publishes `m80-bootstrap-selector.tsv`, a non-executable
line-oriented projection of the canonical JSON index for the no-installed-binary
POSIX installer path. It contains:

- `schema_version`;
- `release_tag`;
- a fixed `columns` row;
- one `row` per index asset with `os`, `arch`, `image_kind`, bundle name/URL,
  bundle sha256, `size_bytes`, metadata name/sha256, checksum name, proof asset
  names, and `m80_version`.

The selector is generated mechanically from `m80-release-assets.json`, is
checksum-covered, appears in `SHA256SUMS`, and is represented in release
integrity material. Selector values are conservative ASCII shell tokens:
whitespace, control characters, and shell metacharacters are invalid; nullable
proof fields use `-`. `signature_name` is nullable in schema v1 because there
is no detached-signature lane, but `attestation_name` is required for official
signed release rows and names `m80-release-integrity.attestation.jsonl`. It is
not a second source of truth: verification compares selector rows back to the
JSON index and rejects stale tag, missing tuple, duplicate tuple, stale
digest/size, unsupported schema, shell-unsafe tokens, or hand-edited drift.

Release publication must generate the index from the actual dist files, upload
it with the rest of the release assets, then re-download the public release and
validate the index against the uploaded tarball, metadata sidecar, checksum
sidecars, build manifest, and `SHA256SUMS`. A new architecture or image kind
is a new row in the index, not a new README quickstart command.

The current publisher is checksum-covered and attestation-referenced:
`signature_name` is null, and `attestation_name` is
`m80-release-integrity.attestation.jsonl`. Checksum coverage proves the exact
bytes that the selector parsed; it is not a substitute for signed release integrity
verification. Signed-release verification fails closed when an
installer-consumed row omits `signature_name` or `attestation_name`, leaves
`attestation_name` empty, names a stale attestation bundle, names a signature
file that is absent from the release dist, or drifts from the release tag,
bundle metadata version, bundle tarball, metadata sidecar, checksum sidecar, or
selector row.
