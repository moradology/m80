# `m80-observability`

VM-lifecycle event log, per-VM probe, health rollup, Prometheus rendering.
The per-VM diagnostics writer is active; probe, health, and scrape remain
reserved for v0.2.

## Reason for being

Two reasons to claim the name now even though the crate is empty:

1. **Stable boundary.** `m80-firecracker` records VM-lifecycle events through
   this crate instead of growing its own diagnostics subsystem. v0.2 will build
   the rollup + scrape on top of the same JSONL primitive.
2. **Explicit split.** Diagnostics JSONL is in scope now because WRA0 needs
   request correlation. Probe, health, and Prometheus rendering stay deferred.

The reframe relative to predecessor: agent-tier semantic events
(`sandbox_exec_started/_succeeded/_failed/_timed_out`,
`runtime_reset_triggered`) and the four-mode escalation
(default/debug/trace_forensics/deep_diagnostic) **do not belong here**.
m80 emits VM-lifecycle events only — boot, ready, stop, delete, host
preflight, storage prepare, etc. Agent semantics layer above m80 in an
adapter, not below.

## Black-box contract

### Diagnostics event log

- Every VM gets a `<run_dir>/diagnostics.jsonl` of structured events.
  Phases: `StartupScavenge`, `HostPreflight`, `StoragePrepare`, `Boot`,
  `NetworkPrepare`, `Ready`, `Request`, `Stop`, `Writeback`, and `Delete`.
- Each line carries `schema_version: 2`, `timestamp_unix_ms`, `source_class`,
  `phase`, `message`, optional opaque `request_id`, and bounded string
  `context`.
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

## Public surface

- `Diagnostics::open(run_dir)`, `Diagnostics::disabled()`,
  `Diagnostics::record(&VmEvent)`, `Diagnostics::path()`.
- `DIAGNOSTICS_SCHEMA_VERSION`, `DIAGNOSTICS_FILE_NAME`.
- `Phase`, `SourceClass`, `VmEvent`.
- `probe(...)`, `VmProbeRecord`, `VmHealth`.
- `aggregate_health(...)`, `HealthSnapshot`, `OpsMetrics`.
- `render_prometheus(...)`, `render_health_json(...)`.
- `ObservabilityError` (`Deferred` remains for probe/health/scrape lanes).

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
- `tempfile` in tests.

## Tests

- JSONL format: opening diagnostics writes schema-versioned events with
  request ids and context.
- Phase vocabulary: the documented lifecycle phase enum serializes to the
  expected names.
- Disabled diagnostics: record remains a no-op.
- Probe classification: a fixture run-root with each {Healthy,
  Degraded, Stuck, Exited} layout produces the expected record kind. (v0.2)
- Rollup determinism: same probe input → same `HealthSnapshot`.
- Prometheus exposition format: validator passes the rendered text. (v0.2)
