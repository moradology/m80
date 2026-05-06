# Health Rollup

Behavior capture for `m80-1f8.3`.

## snapshot-shape

`m80-observability::aggregate_health(records)` produces a deterministic
`HealthSnapshot` from probe records. The snapshot counts healthy, degraded,
stuck, exited, and total records, then derives `rollout_ready`.

`rollout_ready` is true only when there are no degraded, stuck, or exited VMs
in the input set. Empty input is structurally ready: there is no visible VM
residue to block a rollout.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs`
`FirecrackerHealthSnapshot` and `collect_health_snapshot`.

## json-render

`m80-observability::render_health_json(&snapshot)` returns pretty JSON for the
snapshot. The function only renders a supplied snapshot; callers decide where
to store or ship it.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs`
`render_health_snapshot`.

## Verification

- `crates/m80-observability/src/health.rs::tests::aggregate_counts_each_health_class`
- `crates/m80-observability/src/health.rs::tests::render_health_json_is_pretty_json`
