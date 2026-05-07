# Binary Discovery

`m80-preflight` resolves the Firecracker and jailer binaries before any VM
launch work begins. Discovery is fail-closed: a missing binary, a failed
`firecracker --version` probe, or a configured version mismatch returns a typed
`PreflightError` and does not append a success row to the host preflight report.

## Resolution

`BinaryDiscoveryConfig::from_env()` recognizes only the exact m80 environment
keys:

- `M80_FIRECRACKER_BIN`
- `M80_JAILER_BIN`
- `M80_JAILER_HARDEN_BIN`
- `M80_FIRECRACKER_VERSION`

When no path override is present, Firecracker defaults to
`/opt/firecracker/bin/firecracker`, jailer defaults to
`/opt/firecracker/bin/jailer`, and the m80 hardening wrapper defaults to
`/opt/m80/bin/m80-jailer-harden`. There are no legacy aliases without the
`M80_` prefix.

## Version Probe

`discover_binaries(&config)` executes `firecracker --version`, parses the last
whitespace-separated token on the first stdout line, and reports it as
`BinaryDiscovery::firecracker_version`. Typical Firecracker output is
`Firecracker v1.15.1`, which yields `v1.15.1`.

If `M80_FIRECRACKER_VERSION` is set through `BinaryDiscoveryConfig::from_env()`
or `expected_firecracker_version` is set directly, the probed version must match
exactly. A mismatch returns
`PreflightError::FirecrackerVersionMismatch { expected, actual }`.

## Evidence

- `crates/m80-preflight/src/binary.rs`
- `crates/m80-preflight/tests/preflight/binary_discovery.rs`
