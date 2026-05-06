# Admission Limiting

## Semaphore

`m80-firecracker::Backend` owns a single synchronous admission semaphore sized
by `BackendConfig::max_concurrent_vms`. Each successful `Backend::admit`
consumes one permit and returns a `Sandbox` in Created state. The permit is
held through Created, Running, and Stopped, then released when the handle is
dropped or the stopped run-dir is deleted/preserved.

This is the m80 cutover from predecessor's async wrapper: there is no
`AdmissionLimitedSandboxBackend` type and no tokio semaphore in
`m80-firecracker` v0.1. Admission is still single-host scope; multi-host
placement is a caller concern.

Test:
- `crates/m80-firecracker/tests/concurrency/admission.rs::permit_acquired_per_vm`

## Max VMs Env

The canonical orchestrator env key is `M80_MAX_CONCURRENT_VMS`. It maps to the
`max_concurrent_vms` config field and defaults to `8` when unset. The
predecessor-style spelling `M80_FIRECRACKER_MAX_CONCURRENT_VMS` is not an alias.

Tests:
- `crates/m80-firecracker/tests/concurrency/admission.rs::reads_max_from_env`
- `crates/m80-firecracker/tests/config_loading.rs::every_documented_env_key_maps_to_its_field`

## Exhausted

Admission fails fast when no permits are available. It does not block waiting
for another VM to exit. The public error is
`FcError::AdmissionRefused { limit }`, rendered as
`admission refused: <limit> concurrent VMs already running`.

Test:
- `crates/m80-firecracker/tests/concurrency/admission.rs::reports_unavailable_when_pool_full`
