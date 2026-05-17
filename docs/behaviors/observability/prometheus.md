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
- `m80_pmem_layers_per_vm_count{sharing="per_vm"|"shared"}`
- `m80_template_count{freshness="fresh"|"invalidated"}`
- `m80_image_store_bytes`
- `m80_template_store_bytes`
- `m80_lease_attribution{template_fingerprint,pmem_digest_set,scratch_source}`

Current histogram families:

- `m80_restore_latency_seconds`
- `m80_post_restore_hook_duration_seconds{hook_variant=...}`

`m80_restore_latency_seconds` uses buckets at `0.05`, `0.1`, `0.15`,
`0.2`, `0.5`, and `1` seconds, plus the Prometheus `+Inf` bucket. The
`0.2` bucket is present because Phase D and Phase F use a <=200ms warm-restore
p99 target.

Layered-rootfs labels are intentionally closed:

- `sharing`: `per_vm` or `shared`.
- `freshness`: `fresh` or `invalidated`.
- `hook_variant`: `reseed_systemd_random_seed`, `regen_machine_id`, or
  `set_hostname`.
- `scratch_source`: `none` or `workspace`.
- `template_fingerprint` and `pmem_digest_set`: bounded validated
  digest/fingerprint label values, not arbitrary caller strings.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs`
`render_prometheus_metrics`.

## Verification

- `crates/m80-observability/src/prometheus.rs::tests::render_prometheus_text_contains_gauges_only`
- `crates/m80-observability/src/prometheus.rs::tests::restore_latency_histogram_has_target_buckets`
- `crates/m80-observability/src/health.rs::tests::metric_label_value_rejects_free_string_shapes`
- `crates/m80-observability/tests/guest_metrics_prometheus.rs::prometheus_render_spec_compliant`
