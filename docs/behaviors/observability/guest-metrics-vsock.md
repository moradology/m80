# Guest metrics over vsock

Bead: `m80-1f8.4.2`

## Behavior

`m80-guestd` accepts a direct `metrics_request` envelope on the same
length-prefixed protobuf vsock protocol as exec, PTY, shutdown, and file
operations. The request has an empty fixed-shape payload. The response is a
`metrics_response` envelope with
typed CPU counters, memory gauges, and guestd request/error counters.

CPU fields are sampled from the aggregate `cpu` line in `/proc/stat` and are
reported as Linux clock ticks. Memory fields are sampled from `/proc/meminfo`
and are reported as bytes. Guestd counters are process-local and reset when the
daemon restarts.

## Host surface

`RunningSandbox::guest_metrics()` sends `MetricsRequest` to the running guest
and returns `m80_proto::MetricsResponse`. The method updates the sandbox idle
activity timestamp like other guest RPCs and returns `FcError::IdleTimedOut` if
the sandbox was already marked idle-expired.

## Prometheus surface

`m80-observability::OpsMetrics::guest` carries one sampled
`MetricsResponse`. `render_prometheus` renders guest values with the
`m80_guest_` prefix:

- `m80_guest_cpu_*_ticks` as counters.
- `m80_guest_mem_*_bytes` as gauges.
- `m80_guest_requests_total` and `m80_guest_errors_total` as counters.

The scrape renderer consumes one optional guest sample. Aggregating guest
metrics across VMs remains out of scope for this bead.

## Regression coverage

- `crates/m80-proto/tests/metrics_round_trip.rs` pins the wire kinds and fixed
  payload round trip.
- `crates/m80-guestd/tests/metrics.rs` drives the connection handler with a
  metrics request and verifies non-zero procfs CPU and memory values.
- `crates/m80-observability/tests/guest_metrics_prometheus.rs` verifies the
  `m80_guest_*` Prometheus families.
