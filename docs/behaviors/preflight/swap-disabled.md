# Swap Disabled Preflight

`m80-preflight` reads `/proc/swaps` and requires the file to contain only its
header line. Active swap entries fail with `PreflightError::SwapActive`, and
the error carries the active device names.

Swap is a hard gate because guest memory paged to host storage can survive VM
teardown as recoverable data. The repair hint points operators at:

```sh
sudo swapoff -a
```

Operators must also remove persistent swap entries from `/etc/fstab` if they
want the repair to survive reboot. `M80_SKIP_CHECK_SWAP=1` records
`skipped by operator` after accepting the data-remanence risk.

Tests:

- `crates/m80-preflight/src/checks_tests.rs::swap_header_only_passes`
- `crates/m80-preflight/src/checks_tests.rs::swap_active_names_devices`
- `crates/m80-preflight/tests/swap_disabled.rs::swap_active_maps_devices_to_host_prerequisite_failure`
