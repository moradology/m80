# SMT Disabled Preflight

`m80-preflight` reads `/sys/devices/system/cpu/smt/control` and records the
host SMT posture in the ordered preflight report.

`off` passes cleanly. Any other value is advisory by default because some
single-tenant operators intentionally keep SMT enabled for throughput.
Setting `M80_SMT_CHECK=fail` turns the same condition into
`PreflightError::SmtEnabled` with stable check id `smt_disabled`. An absent
sysfs file is reported as unknown advisory by default and fails under
`M80_SMT_CHECK=fail`.

Operators can set `M80_SKIP_CHECK_SMT=1` to record `skipped by operator`.

Tests:

- `crates/m80-preflight/src/checks_tests.rs::smt_on_is_advisory_by_default`
- `crates/m80-preflight/src/checks_tests.rs::smt_on_fails_when_configured`
- `crates/m80-preflight/tests/smt_disabled.rs::smt_enabled_hard_fail_maps_to_host_prerequisite_failure`
