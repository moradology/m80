# Binary Discovery

`m80-preflight` resolves the Firecracker and jailer binaries before any VM
launch work begins. Discovery is fail-closed: a relative path, missing binary,
a failed or malformed version probe, a configured version mismatch, a jailer
train mismatch, or a host-binary manifest violation returns a typed
`PreflightError` and does not append a success row to the host preflight
report.

## Resolution

`BinaryDiscoveryConfig::from_env()` recognizes only the exact m80 environment
keys:

- `M80_FIRECRACKER_BIN`
- `M80_FIRECRACKER_SECCOMP_FILTER`
- `M80_JAILER_BIN`
- `M80_JAILER_HARDEN_BIN`
- `M80_NET_HELPER_BIN`
- `M80_FIRECRACKER_VERSION`

When no path override is present, Firecracker defaults to
`/opt/firecracker/bin/firecracker`, Firecracker's advanced seccomp filter
defaults to `/opt/firecracker/bin/firecracker-seccomp-filter.bin`, jailer
defaults to `/opt/firecracker/bin/jailer`, and the m80 hardening wrapper
defaults to `/opt/m80/bin/m80-jailer-harden`. The m80 network helper defaults
to `/opt/m80/bin/m80-net-helper`. There are no legacy aliases
without the `M80_` prefix. All resolved paths must be absolute; an empty env var is
therefore rejected as `PreflightError::NonAbsolutePath`, not treated as "use
the default."

The seccomp filter path is opened with `O_NOFOLLOW` during binary discovery and
must name a non-empty regular file. Missing, non-file, or empty filters fail
preflight before any launch can reach Firecracker.

## Version Probe

`discover_binaries(&config)` executes `firecracker --version` and
`jailer --version`. Both outputs must be the official first-line form:
`Firecracker vMAJOR.MINOR.PATCH` and `Jailer vMAJOR.MINOR.PATCH`.
Unsupported, empty, or malformed output fails closed.

If `M80_FIRECRACKER_VERSION` is set through `BinaryDiscoveryConfig::from_env()`
or `expected_firecracker_version` is set directly, the probed version must match
exactly. A mismatch returns
`PreflightError::FirecrackerVersionMismatch { expected, actual, policy_source }`.
After guest artifact verification, preflight also compares the probed
Firecracker version to the verified guest manifest
`expected_firecracker_version`; the manifest is the runtime train source for a
launch.

The official jailer version must exactly match the accepted Firecracker
version. A mismatch returns
`PreflightError::JailerVersionMismatch { expected, actual, policy_source }`.
The pairing rule is owned by
`crates/m80-preflight/src/firecracker_train.rs`.

## Host Binary Integrity

After path discovery and version probing, preflight reads
`<artifact_dir>/host-binaries.manifest.json`. The manifest must contain exactly
one entry for each TCB binary:

- `firecracker`
- `jailer`
- `m80`
- `m80_cli`
- `m80_jailer_harden`
- `m80_net_helper`

For `firecracker`, `jailer`, `m80_jailer_harden`, and `m80_net_helper`, the
manifest path must match the runtime-configured path. Every recorded path is opened with
`O_NOFOLLOW`; preflight hashes the opened file descriptor and compares it to
the manifest sha256. Each binary must be a regular file owned `root:root`, with
mode no broader than `0755`, and without group/world write bits.

The boot-scoped sentinel cache can skip the Firecracker and jailer version
subprocesses and guest-image manifest verification, but it does not skip
host-binary sha256 verification or seccomp-filter path validation.

## Evidence

- `crates/m80-preflight/src/binary.rs`
- `crates/m80-preflight/src/firecracker_train.rs`
- `crates/m80-preflight/src/binary.rs::tests::relative_firecracker_binary_path_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::missing_firecracker_seccomp_filter_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::empty_firecracker_seccomp_filter_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::jailer_version_mismatch_fails_closed`
- `crates/m80-preflight/src/firecracker_train.rs::tests::rejects_malformed_jailer_version_output`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_hash_mismatch_fails_closed`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_path_mismatch`
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_unsafe_permissions`
