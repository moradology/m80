# Host Kernel And Jailer Identity Preflight

`m80-preflight` fails closed before launch when the host kernel or configured
jailer identity cannot support the Firecracker path.

## Host Kernel Floor

Preflight reads `uname -r`, parses the leading `major.minor`, and requires Linux
6.1 or newer. Releases below that floor, and releases that do not parse, return
`PreflightError::HostKernelUnsupported { actual, minimum }`.

## Jailer Identity

Preflight validates the effective `jail_uid` and `jail_gid` that will be handed
to the official jailer. `run()` reads `M80_JAIL_UID` and `M80_JAIL_GID`, both
defaulting to `3000`; `run_with_configs()` callers pass the already-resolved
values in `HostFeaturePreflightConfig`.

Values must parse as decimal `u32`. The numeric UID must resolve via host passwd
lookup and the numeric GID must resolve via host group lookup. Missing identities
return `PreflightError::JailIdentityUnavailable`; invalid numeric input returns
`PreflightError::InvalidJailIdentity`.

## CPU Microcode Row

Preflight reports CPU0 microcode `version` and `processor_flags` when the sysfs
rows are present. Missing rows are reported as `unavailable` in a passing row;
operators can correlate that with CPU vulnerability rows without m80 guessing a
microcode policy.

## Evidence

- `crates/m80-preflight/src/checks.rs`
- `crates/m80-preflight/src/checks_tests.rs::host_kernel_floor_rejects_old_release`
- `crates/m80-preflight/src/checks_tests.rs::jailer_identity_requires_existing_user`
- `crates/m80-preflight/src/checks_tests.rs::cpu_microcode_reports_version_and_flags`
- `crates/m80-cli/src/cmds/tests.rs::effective_jail_identity_maps_to_preflight_config`
