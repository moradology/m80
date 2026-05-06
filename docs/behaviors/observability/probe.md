# Per-VM Probe

Behavior capture for `m80-1f8.2`.

## run-root-walk

`m80-observability::probe(run_root)` walks `<run_root>/*/` and emits one
`VmProbeRecord` for each directory that contains `ownership.lock`. Directories
without an ownership marker are skipped as foreign or incomplete residue.

Each record contains:

- `vm_id`
- `run_dir`
- parsed `ownership_pid`
- whether the owner pid is live under `/proc`
- whether `firecracker.sock` is visible under the run-dir tree
- whether `vsock.sock` is visible under the run-dir tree
- whether `diagnostics.jsonl` exists
- derived `health`

Missing run roots return an empty record list. The probe is read-only and does
not repair, delete, or rewrite run-root state.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/probe.rs`
`collect_probe_snapshot`.

## host-visible-only

Probe health is derived only from host-visible evidence:

- `Exited` — no live ownership pid
- `Healthy` — live owner plus both API and vsock sockets visible
- `Degraded` — live owner plus only one expected socket visible
- `Stuck` — live owner and neither expected socket visible

m80 does not scrape logs or metrics to classify health. Logs explain incidents;
they are not liveness authority.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/probe.rs`
`derive_probe_health`.

## Verification

- `crates/m80-observability/src/probe.rs::tests::probe_walks_owned_run_dirs_only`
- `crates/m80-observability/src/probe.rs::tests::health_classification_uses_host_visible_truth_only`
