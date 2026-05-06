# Authority Boundary

## Evidence Not Truth

`m80-firecracker` does not advance placement state. It owns local VM lifecycle
mechanics: launch, ready, exec, stop, force kill, extract, delete, preserve, and
stale run-root recovery. Those operations produce typed results and diagnostics
that a host controller can consume as evidence.

The backend does not decide whether an agent run is releasable, whether a
workspace may be written back, or whether a higher-level placement lease can be
returned. Those decisions belong to a future adapter or caller. This keeps m80
generic and prevents VM cleanup from smuggling in agent semantics.

Test:
- `crates/m80-firecracker/tests/cleanup/teardown_phase_order.rs::backend_emits_evidence_only`
