# Binary Discovery

`m80-preflight` resolves the Firecracker and jailer binaries before any VM
launch work begins. Discovery is fail-closed: a relative path, missing binary,
a failed `firecracker --version` probe, a configured version mismatch, or a
host-binary manifest violation returns a typed `PreflightError` and does not
append a success row to the host preflight report.

## Resolution

`BinaryDiscoveryConfig::from_env()` recognizes only the exact m80 environment
keys:

- `M80_FIRECRACKER_BIN`
- `M80_FIRECRACKER_SECCOMP_FILTER`
- `M80_JAILER_BIN`
- `M80_JAILER_HARDEN_BIN`
- `M80_FIRECRACKER_VERSION`

When no path override is present, Firecracker defaults to
`/opt/firecracker/bin/firecracker`, Firecracker's advanced seccomp filter
defaults to `/opt/firecracker/bin/firecracker-seccomp-filter.bin`, jailer
defaults to `/opt/firecracker/bin/jailer`, and the m80 hardening wrapper
defaults to `/opt/m80/bin/m80-jailer-harden`. There are no legacy aliases
without the `M80_` prefix. All four resolved paths must be absolute; an empty env var is
therefore rejected as `PreflightError::NonAbsolutePath`, not treated as "use
the default."

The seccomp filter path is opened with `O_NOFOLLOW` during binary discovery and
must name a non-empty regular file. Missing, non-file, or empty filters fail
preflight before any launch can reach Firecracker.

## Version Probe

`discover_binaries(&config)` executes `firecracker --version`, parses the last
whitespace-separated token on the first stdout line, and reports it as
`BinaryDiscovery::firecracker_version`. Typical Firecracker output is
`Firecracker v1.15.1`, which yields `v1.15.1`.

If `M80_FIRECRACKER_VERSION` is set through `BinaryDiscoveryConfig::from_env()`
or `expected_firecracker_version` is set directly, the probed version must match
exactly. A mismatch returns
`PreflightError::FirecrackerVersionMismatch { expected, actual }`.

## Host Binary Integrity

After path discovery and Firecracker version probing, preflight reads
`<artifact_dir>/host-binaries.manifest.json`. The manifest must contain exactly
one entry for each TCB binary:

- `firecracker`
- `jailer`
- `m80`
- `m80_cli`
- `m80_jailer_harden`

For `firecracker`, `jailer`, and `m80_jailer_harden`, the manifest path must
match the runtime-configured path. Every recorded path is opened with
`O_NOFOLLOW`; preflight hashes the opened file descriptor and compares it to
the manifest sha256. Each binary must be a regular file owned `root:root`, with
mode no broader than `0755`, and without group/world write bits.

The boot-scoped sentinel cache can skip the Firecracker version subprocess and
guest-image manifest verification, but it does not skip host-binary sha256
verification or seccomp-filter path validation.

## Evidence

- `crates/m80-preflight/src/binary.rs`
- `crates/m80-preflight/src/binary.rs::tests::relative_firecracker_binary_path_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::missing_firecracker_seccomp_filter_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::empty_firecracker_seccomp_filter_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_path_mismatch`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_unsafe_permissions`
