# Nested Virtualization Disabled Preflight

`m80-preflight` reads the host KVM nested-virtualization controls:

- `/sys/module/kvm_intel/parameters/nested`
- `/sys/module/kvm_amd/parameters/nested`

Values `Y`, `y`, or `1` fail with
`PreflightError::NestedVirtEnabled { vendor }` and stable check id
`nested_virt_disabled`. Values such as `N` or `0` pass. If both files are
absent, this check passes because the earlier KVM availability checks own
whether the host can run Firecracker at all.

Nested virtualization is a hard gate because m80's Firecracker threat model
does not assume guests can themselves act as hypervisors. Operators can set
`M80_SKIP_CHECK_NESTED_VIRT=1` to record `skipped by operator` after accepting
that risk.

Tests:

- `crates/m80-preflight/src/checks_tests.rs::nested_virt_absent_or_disabled_passes`
- `crates/m80-preflight/src/checks_tests.rs::nested_virt_enabled_names_vendor`
- `crates/m80-preflight/tests/nested_virt_disabled.rs::nested_virt_enabled_maps_vendor_to_host_prerequisite_failure`
