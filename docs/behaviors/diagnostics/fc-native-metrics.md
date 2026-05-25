# Firecracker Native Metrics

## Behavior

m80 configures Firecracker's native JSON metrics sink for every cold launch and
snapshot restore before boot resources are PUT or a snapshot is loaded. The
metrics file lives inside the jail root at `/firecracker-metrics.jsonl`; callers
can locate it from a running VM with `RunningSandbox::fc_metrics_path()`, or
from a known run directory with `fc_metrics_path(run_dir, firecracker_bin)`.

The launch phase creates or truncates the host-side file before `PUT /metrics`,
sets mode `0600`, rejects final symlinks with `O_NOFOLLOW`, chowns it to the
jailed Firecracker uid/gid when needed, and passes the jail-visible path in
`MetricsConfig { metrics_path }`.

This only establishes Firecracker's native metrics output file. m80 does not
parse, aggregate, scrape, or translate those counters in this behavior. Guestd
metrics over vsock and Prometheus rendering remain separate observability
surfaces.

## Failure

If file preparation fails, launch fails in `phase_10b_fc_diagnostics` with
`FcError::PathIo`. If Firecracker rejects the metrics config, launch fails with
the typed client error `ClientError::MetricsWriteFailed` wrapped by
`FcError::Client`.

## Tests

- `crates/m80-firecracker-client/tests/metrics_config_round_trip.rs` pins the
  `PUT /metrics` request path, JSON shape, and typed `MetricsWriteFailed`
  error mapping.
- `crates/m80-firecracker/src/launch/tests.rs::phase_10b_fc_metrics_creates_jail_file_and_puts_metrics_config`
  pins host file creation, mode, and metrics PUT wiring without KVM.
- `crates/m80-firecracker/src/launch/tests.rs::phase_10b_fc_diagnostics_puts_logger_then_metrics`
  pins the combined preboot diagnostics phase order.
- `crates/m80-firecracker/src/types.rs::tests::running_sandbox_fc_metrics_path_uses_jail_root_layout`
  pins the `RunningSandbox::fc_metrics_path()` layout rule.
- Real KVM:
  `crates/m80-firecracker/tests/fc_native_metrics_real_kvm.rs::fc_native_metrics_file_exists_after_launch`
  proves the metrics file exists after a successful launch.
