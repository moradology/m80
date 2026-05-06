# Backend Construction

## Contract

`m80-firecracker::Backend` is constructed once from a fully resolved
`BackendConfig`. The caller supplies a `m80_preflight::Discovery` that already
contains resolved Firecracker paths, boot artifacts, run-root, and privilege
status. `Backend::new` does not rerun preflight.

The constructed backend owns:

- the resolved `BackendConfig`
- the admission semaphore sized by `max_concurrent_vms`
- the effective configuration snapshot used for diagnostics

Every `Backend::admit` call on the same backend shares that admission semaphore
and the same resolved discovery paths. Per-request VM details are carried only
by `SandboxConfig`; they do not trigger binary/artifact rediscovery.

## Reuse

Services and CLI flows should build a backend once per process or command
execution boundary, then reuse that handle for subsequent lifecycle operations.
For the CLI process-wrapper path, `m80 run` builds one backend for the command
invocation and admits one sandbox from it. Resident warm-owner work must keep
the owner backend visible and explicit; it must not hide rediscovery inside each
warm lease.

## Evidence

- `crates/m80-firecracker/tests/backend_construction.rs::backend_reused_across_admissions_shares_admission_state`
  proves repeated admissions on one backend share semaphore state and preserve
  the resolved discovery/run-root values.
- `crates/m80-firecracker/tests/admission_semaphore.rs` covers permit limits,
  refused admission, permit return on drop, and permit return after failed
  launch.
