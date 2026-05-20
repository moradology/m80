# Release Asset Index

The release asset index is the machine-readable contract that lets
quickstart/bootstrapper code select a bundle without inferring semantics from a
filename. It is consumed before bundle download for the common path and is not
used when an operator provides an explicit bundle URL override.

The index uses `schema_version: 1` and has one top-level `release_tag`. Every
bundle row records:

- `name` and `url`;
- `sha256` and `size_bytes` for the bundle tarball;
- `metadata_name` and `metadata_sha256` for the byte-identical bundle metadata
  sidecar;
- `checksum_name`, plus optional `signature_name` and `attestation_name`
  references;
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

Release dev builds do not select release assets from the index. A dev build
needs an explicit local bundle URL or artifact override so it cannot silently
consume the mutable public latest release. Explicit bundle URLs bypass index
selection but do not bypass later bundle metadata and integrity verification.

The parser rejects unknown fields, unsupported schemas, empty required fields,
non-SHA-256 digest strings, zero size/schema/protocol numbers, and rows whose
`target` does not match `os` plus `arch`. This keeps the index strict enough
for future OS, architecture, or image-kind expansion without changing the
README quickstart command.
