# TCB Host Binary Integrity

`m80-preflight` verifies the host-side TCB binaries against an install-time
manifest before launch can proceed. The manifest lives at
`<artifact_dir>/host-binaries.manifest.json` and records `firecracker`,
`jailer`, `m80`, `m80_jailer_harden`, and `m80_net_helper`. The
same manifest records `firecracker_seccomp_filter` under `launch_material`
because it is launch-critical input but not an executable binary.

The Firecracker, jailer, and hardening-wrapper manifest paths must match the
runtime-configured paths. The Firecracker seccomp-filter launch-material path
must match the runtime-configured seccomp-filter path. m80 is verified from
the manifest path because it is an install artifact, not a per-launch path knob.

Each binary and launch-material file is opened with `O_NOFOLLOW`, hashed from
the opened file descriptor, and compared with the manifest sha256. Preflight
also rejects non-regular files, anything not owned `root:root`, modes broader
than `0755`, and group/world write bits. Launch-material files must also be
non-empty. A binary that returns the expected
`firecracker --version` string but has different bytes fails with
`PreflightError::BinaryHashMismatch`; a seccomp filter whose bytes changed
fails with `PreflightError::HostLaunchMaterialHashMismatch`. Recorded versions
must also match live Firecracker/jailer discovery, m80 helper `--version`
output, and the seccomp filter's owning Firecracker train.

The preflight sentinel cache does not bypass this check. Host binary integrity
is recomputed on every preflight invocation.

Evidence:

- `crates/m80-image-manifest/tests/host_binaries_manifest.rs`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_helper_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_firecracker_version_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_path_mismatch`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_unsafe_permissions`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_symlink_fails_no_follow_open`
