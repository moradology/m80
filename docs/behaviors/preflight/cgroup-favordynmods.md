# cgroup favordynmods Preflight

`m80-preflight` reuses the host kernel release row from the existing OS/kernel
checks and emits a cgroup-favordynmods awareness row for Linux kernels `6.1`
and newer. It does not call `uname` a second time.

This check is advisory. m80 cannot reliably detect whether the operator has
already applied either mitigation, so the row names both options:

- remount the cgroup v2 mount with `favordynmods`
- boot with `kvm.nx_huge_pages=never`

For kernels older than `6.1`, the classifier emits no row. In a full preflight,
older kernels already fail the host-kernel floor before this advisory stage.

Operators can set `M80_SKIP_CHECK_CGROUP_FAVORDYNMODS=1` to record
`skipped by operator`.

Tests:

- `crates/m80-preflight/src/checks_tests.rs::cgroup_favordynmods_warns_on_kernel_6_1_or_newer`
- `crates/m80-preflight/src/checks_tests.rs::cgroup_favordynmods_absent_on_older_kernel`
- `crates/m80-preflight/src/checks_tests.rs::cgroup_favordynmods_reuses_host_kernel_report_row`
