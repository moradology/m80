# Jailer Harden Root Guard

Behavior capture for `m80-2ggw.2.3`.

Firecracker's official jailer is run as root so it can perform its own mount,
chroot, cgroup, UID, and GID setup. `m80-jailer-harden` now checks effective UID
at the top of `apply_process_hardening()` and returns
`HardenError::NotRoot { actual }` when the wrapper is not running as effective
UID 0.

The check runs before every one-way hardening side effect:

- namespace unshare
- resource limits
- supplementary-group clearing
- capability pruning
- `PR_SET_NO_NEW_PRIVS`
- parent-death signal setup
- umask and signal-mask reset
- inherited-fd closing
- final exec

This keeps a misconfigured non-root wrapper from partially hardening the
process and then handing a broken environment to the official jailer.

Verification:

- `crates/m80-jailer-harden/src/lib.rs::tests::check_not_root_rejects_non_root_euid`
- `crates/m80-jailer-harden/src/lib.rs::tests::check_not_root_accepts_root_euid`
- `crates/m80-jailer-harden/tests/integration_root.rs::wrapper_applies_inherited_hardening_before_exec`
