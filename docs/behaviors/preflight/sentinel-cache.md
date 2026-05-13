# Preflight Sentinel Cache

`m80-preflight` keeps host capability checks live on every invocation, but it
does not need to rerun immutable-artifact work when the host boot and artifact
metadata are unchanged.

After a successful full preflight, m80 writes
`/run/m80-preflight-ok-<sha256>`. The hash input includes the kernel boot id,
the configured Firecracker version pin, the optional kernel-kind override, and
file identity metadata for the Firecracker binary, jailer binary,
`m80-jailer-harden`, selected kernel, selected rootfs, and rootfs manifest.

On a matching sentinel, m80 reuses the cached Firecracker version and manifest,
skipping `firecracker --version` and `Manifest::verify(parent)`. It still runs
OS, KVM, module, cgroup, privilege, run-root, run-root filesystem, and storage
helper checks.

Corrupt sentinels, boot-id changes, rootfs metadata changes, manifest metadata
changes, binary metadata changes, version-pin changes, and kernel-kind override
changes are cache misses. A miss runs the full checks and rewrites the sentinel
only after success. `M80_FORCE_PREFLIGHT=1` disables cache reads and writes for
that invocation.

Tests:

- `crates/m80-preflight/src/cache.rs::tests::corrupt_sentinel_is_ignored_and_rewritten`
- `crates/m80-preflight/src/cache.rs::tests::rootfs_mtime_change_invalidates_sentinel`
- `crates/m80-preflight/src/cache.rs::tests::boot_id_change_invalidates_sentinel`
- `crates/m80-preflight/src/cache.rs::tests::matching_sentinel_reuses_cached_manifest`
- `crates/m80-preflight/src/cache.rs::tests::force_preflight_env_disables_cache_reads_and_writes`
- `crates/m80-preflight/src/binary/tests.rs::cached_firecracker_version_skips_version_subprocess`
- `crates/m80-preflight/src/artifacts/tests.rs::cached_manifest_skips_sha256_verification`
