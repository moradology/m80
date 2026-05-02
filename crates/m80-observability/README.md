# `m80-observability`

VM-lifecycle event log, per-VM probe, health rollup, Prometheus
rendering. **Reserved namespace; deferred to v0.2.** v0.1 ships an empty
crate so v0.2 can fill it without forcing a refactor of consumers.

## Reason for being

Two reasons to claim the name now even though the crate is empty:

1. **Stable boundary.** v0.1's orchestrator (`m80-firecracker`) emits
   `tracing` spans and writes per-VM `diagnostics.jsonl`. v0.2 will
   build the rollup + scrape on top of those primitives. If we leave
   the crate name unclaimed today, every future consumer touches
   `m80-firecracker` directly to wire observability — and m80-firecracker
   becomes a junk drawer.
2. **Explicit deferral.** Reserving the crate signals "this is on the
   roadmap, here's the scope" without forcing v0.1 authors to ship
   half-implementations.

The reframe relative to predecessor: agent-tier semantic events
(`sandbox_exec_started/_succeeded/_failed/_timed_out`,
`runtime_reset_triggered`) and the four-mode escalation
(default/debug/trace_forensics/deep_diagnostic) **do not belong here**.
m80 emits VM-lifecycle events only — boot, ready, stop, delete, host
preflight, storage prepare, etc. Agent semantics layer above m80 in an
adapter, not below.

## Black-box contract (v0.2)

### Diagnostics event log

- Every VM gets a `<run_dir>/diagnostics.jsonl` of structured events.
  Phases: `StartupScavenge`, `HostPreflight`, `StoragePrepare`, `Boot`,
  `Ready`, `Stop`, `Delete`, plus error variants.
- Lines are `serde_json`-encoded, one event per line, append-only,
  fsync-on-close. Crash-tolerant by construction.
- Removing the diagnostics writer never breaks the boot path. The
  orchestrator wraps it in `Option<Diagnostics>`.

### Per-VM probe

- `probe(run_root: &Path) -> Result<Vec<VmProbeRecord>, ObservabilityError>`
  (free function) walks the run-root, reads ownership markers + lease
  files + socket reachability, and emits one record per VM.
- Health classification (`Healthy`, `Degraded`, `Stuck`, `Exited`) is
  derived from **host-visible truth only** — the absence/presence of
  files and sockets — not from log lines or metric values.
- The probe is read-only and idempotent.

### Health rollup

- `aggregate_health(records: &[VmProbeRecord]) -> Result<HealthSnapshot, ObservabilityError>`
  (free function) produces ready/stuck flags suitable for
  rollout-readiness gating.

### Prometheus rendering

- `render_prometheus(snapshot: &HealthSnapshot, metrics: &OpsMetrics) -> String`
  renders the standard exposition format. **Rendering only** — no
  embedded HTTP server. Hosting the `/metrics` endpoint is the
  deployer's job.

## Public surface (v0.2)

- `Diagnostics`, `Phase`, `VmEvent`.
- `probe(...)`, `VmProbeRecord`, `VmHealth`.
- `aggregate_health(...)`, `HealthSnapshot`, `OpsMetrics`.
- `render_prometheus(...)`, `render_health_json(...)`.
- `ObservabilityError` (carries `Deferred` in v0.1 for everything
  except `Diagnostics::record`, which is a no-op `Ok(())`).

In **v0.1** the crate exposes only `Diagnostics::disabled()` so callers
can write `Option<Diagnostics>` against a stable type without conditional
compilation.

## Non-goals

- **No semantic agent events.** `sandbox_exec_*`, `runtime_reset_*`,
  Stage-H schemas, four-mode escalation — all out of scope. They live
  in an adapter.
- **No log shipping.** This crate writes locally; getting events off
  the host is the deployer's concern.
- **No HTTP server.** Rendering only.
- **No alerting.** Rollup is structural, not policy.

## Dependencies

- `serde`, `serde_json`.
- `thiserror`.
- (No other m80 crates in v0.1. v0.2 may depend on
  `m80-firecracker` for run-root layout types.)

## Tests (v0.2)

- Append-only crash-tolerance: kill the diagnostics writer mid-frame;
  the partial frame is recovered or skipped without breaking parse.
- Probe classification: a fixture run-root with each {Healthy,
  Degraded, Stuck, Exited} layout produces the expected record kind.
- Rollup determinism: same probe input → same `HealthSnapshot`.
- Prometheus exposition format: `prometheus`-spec validator passes the
  rendered text.
