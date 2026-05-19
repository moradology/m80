# Firecracker Seccomp Filter

`m80-preflight` requires a Firecracker advanced seccomp filter before VM launch.
The filter is the compiled bitcode output produced by Firecracker's matching
`seccompiler-bin`, not the source JSON policy. The default host path is
`/opt/firecracker/bin/firecracker-seccomp-filter.bin`; operators can override
it with `M80_FIRECRACKER_SECCOMP_FILTER`.

The path must be absolute. Discovery opens it with `O_NOFOLLOW`, rejects
missing paths, non-regular files, and empty files, and records the resolved path
in `Discovery::firecracker_seccomp_filter`.

The installed `host-binaries.manifest.json` records the same path and sha256
under `launch_material.firecracker_seccomp_filter`, along with the Firecracker
train the filter belongs to. Preflight verifies that entry on every invocation:
path match, `O_NOFOLLOW` open, regular file, non-empty contents, `root:root`
ownership, safe mode bits, sha256 match, and version match against the accepted
Firecracker train. The seccomp filter is launch material, not a host binary.

`m80-firecracker` treats that path as launch material. Phase 4 binds it
read-only into the jail as `firecracker-seccomp-filter.bin`, and
`m80-jailer` passes `--seccomp-filter firecracker-seccomp-filter.bin` after
the jailer `--` separator so the argument reaches Firecracker, not the jailer.

The boot-scoped preflight cache includes the filter file identity and still
validates the filter path on cache hits. Replacing, emptying, or retargeting
the filter therefore invalidates cached discovery rather than letting an older
successful preflight authorize a different filter.

This behavior covers the VMM process. Guest workload seccomp is a separate
guestd/exec-shim hardening surface. The required broker split is captured in
`docs/design/security-hardening-boundary-splits.md`.

## Evidence

- `crates/m80-preflight/src/binary.rs::tests::missing_firecracker_seccomp_filter_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::empty_firecracker_seccomp_filter_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::relative_firecracker_seccomp_filter_path_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_version_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_launch_material_symlink_fails_no_follow_open`
- `crates/m80-jailer/src/materialized.rs::tests::launch_redirects_stdio_and_passes_hardening_args`
