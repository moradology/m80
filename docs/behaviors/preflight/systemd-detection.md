# systemd Launch Detection

Preflight chooses the host launch path before the Firecracker launch site runs.
The selection is explicit in `Discovery::chosen_launch_path`:

- `LaunchPath::Systemd` when `systemd-run` is executable, reports
  `systemd >= 245`, and can create a no-op transient unit with
  `systemd-run --collect --wait --pipe --property=NoNewPrivileges=yes /bin/true`;
- `LaunchPath::Wrapper` when the systemd probe fails and
  `m80-jailer-harden` is present;
- `PreflightError::LaunchPathUnavailable` when neither path is usable.

When both paths are usable, preflight records systemd as the selected path and
reports the wrapper as fallback material. It does not treat the two paths as
equivalent: downstream launch code reads `chosen_launch_path` and follows the
single selected path for that preflight result.

The systemd probe resolves and stores the absolute `systemd-run` path in
`Discovery::systemd_run_bin`. Launch code must use that resolved path rather
than searching `PATH` again.

## Evidence

- `crates/m80-preflight/src/checks.rs`
- `crates/m80-preflight/src/checks_tests.rs`
- `crates/m80-preflight/tests/systemd/detection.rs`
