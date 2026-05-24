# KSM Disabled Preflight

`m80-preflight` reads `/sys/kernel/mm/ksm/run` before binary or artifact work.
When the file is present, only `0` passes. Any other value fails with
`PreflightError::KsmEnabled` and maps to the stable check id `ksm_disabled`.

KSM is a hard gate because page deduplication can create cross-VM memory
side channels on shared hosts. The repair hint tells operators to run:

```sh
echo 0 | sudo tee /sys/kernel/mm/ksm/run
```

If the sysfs file is absent, preflight records that KSM is unavailable and
continues. Operators can set `M80_SKIP_CHECK_KSM=1` to record
`skipped by operator` after accepting the risk.

Tests:

- `crates/m80-preflight/src/checks_tests.rs::ksm_enabled_fails_closed`
- `crates/m80-preflight/tests/ksm_disabled.rs::ksm_enabled_maps_to_host_prerequisite_failure`
