# Firecracker Native Logger

## Behavior

m80 configures Firecracker's native structured logger for every cold launch and
snapshot-restore launch before the VM is started or restored.

After the Firecracker API socket opens, `m80-firecracker` creates
`firecracker.log` inside the jail root with mode `0600`, assigns it to the
jailed Firecracker uid/gid, and sends `PUT /logger` with the jail-visible path
`/firecracker.log`. The default level is `Warning`; callers can override it
with `SandboxConfig::fc_log_level`.

The native logger complements `console.log`. `console.log` carries guest serial
output and process stderr. `firecracker.log` carries Firecracker-side device,
MMIO, vhost, API, and internal VMM events that may explain failures before the
guest can print anything useful.

## Run-Dir Contract

The host-visible path is computed by `fc_log_path(run_dir, firecracker_bin)`.
The file lives under the jail root, not directly under the run directory,
because Firecracker is already chrooted when it opens the path supplied to
`PUT /logger`.

`StoppedSandbox::delete` removes the file with the rest of the run directory.
`StoppedSandbox::preserve_for_triage` preserves it for offline inspection.

## Tests

- `crates/m80-firecracker-client/tests/logger_config_round_trip.rs` pins the
  `PUT /logger` request path, JSON shape, optional-field omission, and typed
  `LoggerWriteFailed` error mapping.
- `crates/m80-firecracker/src/launch/tests.rs::phase_10b_fc_logger_creates_jail_file_and_puts_logger_config`
  pins the host file creation and logger PUT wiring without KVM.
- The ignored real-KVM smoke test
  `crates/m80-firecracker/tests/fc_native_logger_real_kvm.rs::fc_native_logger_file_exists_after_launch`
  verifies a real launched VM has the jail-root log file.
