# Production Metrics Surface

Behavior capture for bead `m80-mrjqs.3`.

## Contract

`m80-observability` exposes its probe, health rollup, and Prometheus renderer
in the default public surface. There is no `_test_internal` feature gate for:

- `probe`, `VmProbeRecord`, and `VmHealth`.
- `aggregate_health`, `HealthSnapshot`, and `OpsMetrics`.
- `render_prometheus` and `render_health_json`.

`m80-firecracker` owns process-local VM mechanics counters. The counters are
stored as atomics and reset when the embedding process exits:

- successful launches: `m80_launches_total`;
- phase failures by phase name: `m80_phase_failures_total{phase=...}`;
- errors by finite `FcError` variant: `m80_errors_total{variant=...}`;
- vsock disconnects before required terminal frames:
  `m80_vsock_disconnects_total`;
- idle watcher expirations: `m80_idle_timeout_total`.

Every `phase_result` error records the phase failure and, when the error is an
`FcError`, the finite variant label. Warm-pool metrics are derived from
`WarmPoolSnapshot` with `warm_pool_metrics(snapshot)` and rendered as depth
gauges plus fill/lease/discard counters.

## CLI

`m80 metrics` renders Prometheus exposition text from the configured run-root
health probe, an active warm owner's slot snapshot when one is running, and the
current process-local counter snapshot. It does not start an HTTP server.
Because the counter store is process-local, a standalone `m80 metrics` process
does not recover historical launch/error counters from prior short-lived CLI
invocations; embedders that keep m80 in-process should call
`m80_firecracker::ops_metrics_snapshot()` and render that snapshot in the
serving process.

Default mode writes to stdout:

```text
m80 metrics
```

Textfile-exporter mode atomically replaces the supplied path:

```text
m80 metrics --textfile /var/lib/node_exporter/textfile_collector/m80.prom
```

The textfile parent directory must already exist. Missing directories fail
closed instead of being created implicitly. Global `--json` is invalid because
Prometheus text is the command wire format.

## Verification

- `cargo test -p m80-observability`
- `cargo test -p m80-firecracker --lib ops_metrics`
- `cargo test -p m80-cli metrics`
- `cargo test -p m80-cli --test parse_args parse_metrics_textfile_shape`
