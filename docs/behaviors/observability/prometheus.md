# Prometheus Rendering

Behavior capture for `m80-1f8.4`.

## render-only

`m80-observability::render_prometheus(&health, &metrics)` renders Prometheus
text exposition from supplied health and ops metrics. The crate provides
rendering only. It does not bind an HTTP port, run a server, scrape on an
interval, or decide alert policy.

Current gauge families:

- `m80_vm_health_healthy`
- `m80_vm_health_degraded`
- `m80_vm_health_stuck`
- `m80_vm_health_exited`
- `m80_vm_health_total`
- `m80_vm_rollout_ready`
- `m80_ops_vm_count`

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs`
`render_prometheus_metrics`.

## Verification

- `crates/m80-observability/src/prometheus.rs::tests::render_prometheus_text_contains_gauges_only`
