# KVM Timer Floor Preflight

`m80-preflight` reads `/sys/module/kvm/parameters/min_timer_period_us` and
records the host KVM PIT timer floor posture in the ordered report.

The check is advisory. Value `0` records a warning because Firecracker's
production host setup guidance recommends evaluating `500` to avoid guests
requesting extremely short PIT/HPET timer intervals and flooding the host with
timer interrupts. Nonzero values pass with the observed value. An absent sysfs
file records that the timer floor could not be evaluated.

Operators can set `M80_SKIP_CHECK_KVM_TIMER=1` to record
`skipped by operator`.

Tests:

- `crates/m80-preflight/src/checks_tests.rs::kvm_timer_floor_reports_zero_as_warning`
- `crates/m80-preflight/src/checks_tests.rs::kvm_timer_floor_absent_warns`
- `crates/m80-preflight/tests/kvm_timer_floor.rs::kvm_timer_floor_unset_maps_to_host_prerequisite_failure`
