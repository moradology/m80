# `m80-observability`

VM-lifecycle event log, per-VM probe, health rollup, Prometheus rendering.
The per-VM diagnostics writer, run-root probe, health rollup, and Prometheus
rendering are active generic VM-observability surfaces.

## Reason for being

Two reasons to keep this as a separate crate:

1. **Stable boundary.** `m80-firecracker` records VM-lifecycle events through
   this crate instead of growing its own diagnostics subsystem. Probe, health,
   and scrape build on the same host-visible run-root evidence.
2. **Explicit split.** Diagnostics JSONL is in scope because WRA0 needs
   request correlation. Probe, health, and Prometheus rendering stay
   VM-generic and do not become product policy.

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
- Each line carries `schema_version: 2`, `timestamp_unix_ms`, `event_kind`,
  `source_class`, `phase`, `message`, optional opaque `request_id`, bounded
  string `context`, optional `duration_us`, optional `outcome`, and optional
  typed `exit_reason`.
- Phase timings are first-class events: every instrumented phase emits
  `phase_started` and `phase_completed` records with a typed completion
  outcome.
- Stop evidence uses typed `ExitReason` values such as `NormalStop`,
  `ForceKill`, and `SnapshotCapture`.
- Lines are `serde_json`-encoded, one event per line, append-only,
  fsync-on-close. Crash-tolerant by construction.
- Removing the diagnostics writer never breaks the boot path. The
  orchestrator wraps it in `Option<Diagnostics>`.

### Per-VM probe

- `probe(run_root: &Path) -> Result<Vec<VmProbeRecord>, ObservabilityError>`
  (free function) walks the run-root, reads ownership markers and socket
  visibility, and emits one record per owned VM.
- Health classification (`Healthy`, `Degraded`, `Stuck`, `Exited`) is
  derived from **host-visible truth only** — the absence/presence of
  files and sockets — not from log lines or metric values.
- The probe is read-only and idempotent.

### Health rollup

- `aggregate_health(records: &[VmProbeRecord]) -> Result<HealthSnapshot, ObservabilityError>`
  (free function) produces per-health counts and a structural
  `rollout_ready` boolean.

### Prometheus rendering

- `render_prometheus(snapshot: &HealthSnapshot, metrics: &OpsMetrics) -> Result<String, ObservabilityError>`
  renders the standard exposition format. **Rendering only** — no
  embedded HTTP server. Hosting the `/metrics` endpoint is the
  deployer's job.
- `OpsMetrics::guest` optionally carries one `m80_proto::MetricsResponse`
  sampled from a running VM. When present, `render_prometheus` emits
  `m80_guest_cpu_*`, `m80_guest_mem_*`, `m80_guest_requests_total`, and
  `m80_guest_errors_total`.

## Public surface

- `Diagnostics::open(run_dir)`, `Diagnostics::disabled()`,
  `Diagnostics::record(&VmEvent)`, `Diagnostics::path()`.
- `DIAGNOSTICS_SCHEMA_VERSION`, `DIAGNOSTICS_FILE_NAME`.
- `EventKind`, `Phase`, `PhaseOutcome`, `ExitReason`, `SourceClass`,
  `VmEvent`.
- `probe(...)`, `VmProbeRecord`, `VmHealth`.
- `aggregate_health(...)`, `HealthSnapshot`, `OpsMetrics`.
- `render_prometheus(...)`, `render_health_json(...)`.
- `ObservabilityError`.

## Non-goals

- **No semantic agent events.** `sandbox_exec_*`, `runtime_reset_*`,
  Stage-H schemas, four-mode escalation — all out of scope. They live
  in an adapter.
- **No log shipping.** This crate writes locally; getting events off
  the host is the deployer's concern.
- **No HTTP server.** Rendering only.
- **No alerting.** Rollup is structural, not policy.

## Dependencies

- `m80-proto`.
- `serde`, `serde_json`.
- `thiserror`.
- `tempfile` in tests.

## Tests

- JSONL format: opening diagnostics writes schema-versioned events with
  request ids and context.
- Phase vocabulary: the documented lifecycle phase enum serializes to the
  expected names.
- Disabled diagnostics: record remains a no-op.
- Phase timing events: completed phases carry duration and typed outcome.
- Stop evidence: normal stop, force kill, and snapshot capture serialize as
  distinct typed reasons.
- Probe classification: a fixture run-root with each {Healthy,
  Degraded, Stuck, Exited} layout produces the expected record kind.
- Rollup determinism: same probe input → same `HealthSnapshot`.
- Prometheus exposition format: rendered text contains VM health gauges,
  operational gauges, and guest-side counter/gauge families when a guest sample
  is present.
