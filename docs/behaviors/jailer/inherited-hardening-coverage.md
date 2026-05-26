# Inherited hardening coverage

Bead: `m80-vpw49.2`

This behavior capture records the no-KVM and root-only tests that pin
`m80-jailer-harden` and `m80-jailer` inherited process hardening.

## Capability pruning

The wrapper keeps only the official Firecracker jailer setup capabilities in
the bounding, permitted, and effective sets:

- `CAP_CHOWN`
- `CAP_DAC_OVERRIDE`
- `CAP_SYS_CHROOT`
- `CAP_MKNOD`
- `CAP_SETUID`
- `CAP_SETGID`
- `CAP_SYS_ADMIN`

Pre-jailer host capabilities such as `CAP_NET_ADMIN`, `CAP_KILL`,
`CAP_FOWNER`, `CAP_SYS_PTRACE`, `CAP_SYS_MODULE`, `CAP_SYS_RAWIO`, and
`CAP_SETPCAP` do not survive into the official jailer exec.

Tests:
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
