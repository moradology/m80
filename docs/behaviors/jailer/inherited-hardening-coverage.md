# Inherited hardening coverage

Bead: `m80-vpw49.2`

This behavior capture records the tests and launch contracts that pin
inherited process hardening before Firecracker's official jailer executes.
On supported systemd hosts this hardening is applied by the transient
`systemd-run` VM unit. On hosts without supported systemd it is applied by
the feature-gated `m80-jailer-harden` fallback wrapper.

## Launch Path Split

`m80-preflight` chooses exactly one host launch path:

- `LaunchPath::Systemd` starts the official jailer through a transient unit.
  The VM unit bounds capabilities to the official jailer setup set, clears
  ambient capabilities, sets `NoNewPrivileges=yes`, resets supplementary
  groups and environment, sets `UMask=0077`, uses `KeyringMode=private`,
  restricts address families, locks personality, enables kernel-interface
  protections, mirrors resource limits, and routes stdout/stderr according to
  the configured console log.
- `LaunchPath::Wrapper` uses `m80-jailer-harden` before the official jailer.
  That path preserves fallback coverage and its root integration tests.

Neither path owns the official jailer's final `exec` site. Final-exec-site
capability/seccomp hardening remains future work under `m80-92eor`.

## Capability pruning

Both launch paths keep only the official Firecracker jailer setup capabilities
in the bounding set:

- `CAP_CHOWN`
- `CAP_DAC_OVERRIDE`
- `CAP_SYS_CHROOT`
- `CAP_MKNOD`
- `CAP_SETUID`
- `CAP_SETGID`
- `CAP_SYS_ADMIN`

Pre-jailer host capabilities such as `CAP_NET_ADMIN`, `CAP_KILL`,
`CAP_FOWNER`, `CAP_SYS_PTRACE`, `CAP_SYS_MODULE`, `CAP_SYS_RAWIO`, and
`CAP_SETPCAP` do not survive into the official jailer exec on the wrapper path.
The systemd path also clears ambient capabilities and applies the same bounding
set before starting the official jailer.

Tests:
- `crates/m80-firecracker/src/launch/systemd.rs::tests::vm_launch_directive_snapshot_is_pinned`
- `crates/m80-jailer-harden/tests/integration_root.rs::wrapper_applies_inherited_hardening_before_exec`
- `crates/m80-jailer/tests/integration_root.rs::launch_with_new_pid_ns_records_sentinel_and_firecracker_is_pid_one`

## Inherited file descriptors

`m80-jailer-harden` closes inherited file descriptors from fd 3 through
`UINT_MAX` before execing the official jailer. Fds 0, 1, and 2 stay open. The
operation succeeds when there is no explicit extra fd to close; Linux
`close_range(3, UINT_MAX, 0)` ignores already-closed descriptors in the range.

Tests:
- `close_inherited_fds_preserves_stdio_and_closes_extra_fd`
- `close_inherited_fds_succeeds_without_extra_fd`
- `wrapper_applies_inherited_hardening_before_exec`

## Jailer plan and recovery edges

The m80-jailer side already covers the absorbed plan/recovery gaps from
`m80-vpw49.3`: rejected bind destinations, basename validation, and orphan
recovery decisions for incomplete or stale state.

Tests:
- `crates/m80-jailer/tests/plan_compute.rs`
- `crates/m80-jailer/tests/recover.rs`

## Evidence

- `docs/behaviors/launch/systemd-directives.md`
- `docs/decisions/0010-systemd-launch-default.md`
- `crates/m80-firecracker/src/launch/systemd.rs`
- `crates/m80-jailer-harden/README.md`
