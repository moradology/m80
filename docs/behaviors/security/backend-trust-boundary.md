# Backend Trust Boundary

Bead: `m80-8emae.35`

`m80-firecracker` supports Rust library embedding, but the library boundary is
not a security boundary. Code running in the same process can inspect heap
state, retain handles, call public methods, and use documented run-root paths.
Untrusted adapter code needs a separate process or service boundary around m80.

`BackendConfig` is not literal-constructible outside `m80-firecracker`.
External callers construct it with `BackendConfig::builder(discovery)`, where
`discovery` is the preflight result. The resulting config exposes read-only
accessors for diagnostics and tests.

Caller-supplied VM IDs are admitted only when they are 1..=64 ASCII
alphanumeric, `.`, `_`, or `-` characters, are not `.` / `..`, are not
reserved run-root names, and fit the AF_UNIX socket path budget. Shape and
reserved-name failures surface as `FcError::InvalidVmId`; over-budget values
surface as `FcError::Config(ConfigError::VmIdPathBudgetExceeded)`. Stale
run-root recovery applies the same shape rule before treating a child
directory name as a VM ID; malformed names are preserved for manual inspection
and are not passed to cgroup or network cleanup.

Tests:

- `crates/m80-firecracker/tests/security/backend_trust_boundary.rs::caller_vm_id_shape_fails_before_admission_permit`
- `crates/m80-firecracker/tests/security/backend_trust_boundary.rs::startup_recovery_preserves_invalid_run_root_child_names`
