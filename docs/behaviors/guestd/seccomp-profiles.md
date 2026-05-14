# Guestd Seccomp Profiles

Behavior capture for bead `m80-8emae.14.3`.

After PID-1 setup, vsock listener bind, ready signaling, and workload broker
seam initialization, long-lived `m80-guestd` installs a fixed daemon seccomp
profile. Workload children then enter the existing hidden exec shim, drop to
UID/GID 1000 with `PR_SET_NO_NEW_PRIVS` and empty capabilities, and stack a
fixed workload seccomp profile before `exec`.

The profiles are not caller-configurable. m80 wire requests do not carry a
seccomp policy selector, extension list, or adapter-specific tool policy. The
daemon profile denies kernel-privilege primitives that guestd does not need
after readiness. The workload profile additionally denies namespace, mount,
swap, and reboot primitives while preserving ordinary command execution.

This leaf deliberately keeps arbitrary workload exec available under a fixed
deny profile. It is not a public policy API and does not promote adapter
semantics into m80.

Tests:

- `crates/m80-guestd/tests/guestd/seccomp_profiles.rs::daemon_seccomp_probe_enters_filter_mode`
- `crates/m80-guestd/tests/guestd/seccomp_profiles.rs::workload_seccomp_probe_enters_filter_mode`
- `crates/m80-guestd/tests/guestd/seccomp_profiles.rs::workload_seccomp_probe_blocks_denied_namespace_syscall`
- `crates/m80-guestd/tests/guestd/seccomp_profiles.rs::workload_seccomp_probe_preserves_ordinary_exec`
- `crates/m80-guestd/src/guest_seccomp.rs::tests::daemon_profile_does_not_block_workload_filter_install_syscalls`
- `crates/m80-guestd/src/guest_seccomp.rs::tests::workload_profile_denies_namespace_and_mount_primitives`
