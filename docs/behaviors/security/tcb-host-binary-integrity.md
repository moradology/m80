# TCB Host Binary Integrity

`m80-preflight` verifies the host-side TCB binaries against an install-time
manifest before launch can proceed. The manifest lives at
`<artifact_dir>/host-binaries.manifest.json` and records `firecracker`,
`jailer`, `m80`, `m80_cli`, and `m80_jailer_harden`.

The Firecracker, jailer, and hardening-wrapper manifest paths must match the
runtime-configured paths. m80 and m80-cli are verified from the manifest paths
because they are install artifacts, not per-launch path knobs.

Each binary is opened with `O_NOFOLLOW`, hashed from the opened file
descriptor, and compared with the manifest sha256. Preflight also rejects
non-regular files, anything not owned `root:root`, modes broader than `0755`,
and group/world write bits. A binary that returns the expected
`firecracker --version` string but has different bytes fails with
`PreflightError::BinaryHashMismatch`.

The preflight sentinel cache does not bypass this check. Host binary integrity
is recomputed on every preflight invocation.

Evidence:

- `crates/m80-image-manifest/tests/host_binaries_manifest.rs`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_path_mismatch`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_unsafe_permissions`
