# Leaves — Tail (L1-15 Cleanup, L1-16 Snapshot, L1-17 Warm-pool, L1-18 Observability)

This file specifies leaf beads for the four "tail" L1 epics. Per the m80
reframe, snapshot manifest schemas + persistence layout remain contract-required
v0.1 surfaces (Agent 2 §J refutes the dossier "drop entirely" claim);
blank-pool reset vocabulary remains contract-required even though the allocator
is unwired; observability is reduced to VM-lifecycle events only (no
agent-semantic Stage-H events). Cleanup keeps only generic non-release
conditions; the agent-tier writeback dispositions are captured in a future
adapter spec.

---

## L1-15 Cleanup, Drain, Teardown

## L2-15.1 Teardown phase order (parent_var: $L2_15_1)

### Leaf: Drive teardown through admission_fence → bounded_stop → optional_extract → residue_cleanup → release
- parent_var: $L2_15_1
- labels: $ACTIVE,cleanup,phases
- status: open
- behavior: The system orchestrates VM teardown as an explicit phase sequence: admission fence, bounded stop, optional change-extraction, residue cleanup, then host-owned release.
- source: predecessor `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §4 Firecracker Teardown Phases; predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `force_stop_and_cleanup` lines 1308-1339.
- captured-by: m80/docs/behaviors/cleanup/teardown-phase-order.md#phase-sequence + m80/firecracker/tests/cleanup/teardown_phase_order.rs::phases_run_in_documented_order

### Leaf: Fence admission before destructive teardown begins
- parent_var: $L2_15_1
- labels: $ACTIVE,cleanup,phases
- status: open
- behavior: The system requires evidence that the caller has fenced new work and no active backend request remains before any destructive teardown step starts.
- source: predecessor `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §4.1 Admission fence.
- captured-by: m80/docs/behaviors/cleanup/teardown-phase-order.md#admission-fence + m80/firecracker/tests/cleanup/admission_fence.rs::teardown_blocked_until_fenced

### Leaf: Block release when forced kill is ambiguous, cleanup failed, or owned residue may still be live
- parent_var: $L2_15_1
- labels: $ACTIVE,cleanup,non-release
- status: open
- behavior: The system forbids placement release whenever forced kill outcome is ambiguous, owned residue cleanup failed, or any owned runtime artifact may still represent a live VM.
- source: predecessor `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §6 Explicit Non-Release Cases (generic-tier conditions only).
- captured-by: m80/docs/behaviors/cleanup/non-release-conditions.md#generic-conditions + m80/firecracker/tests/cleanup/non_release_conditions.rs::generic_block_set

### Leaf: Treat backend evidence as input, never as placement authority
- parent_var: $L2_15_1
- labels: $ACTIVE,cleanup,authority
- status: open
- behavior: The m80 backend never advances placement state directly; it surfaces stop disposition, residue summary, and cleanup outcome as evidence consumed by a host-owned controller.
- source: predecessor `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §2 Authority Boundary.
- captured-by: m80/docs/behaviors/cleanup/authority-boundary.md#evidence-not-truth + m80/firecracker/tests/cleanup/authority_boundary.rs::backend_emits_evidence_only

## L2-15.2 Arch-sensitive stop (parent_var: $L2_15_2)

### Leaf: Use graceful-then-force stop on x86_64
- parent_var: $L2_15_2
- labels: $ACTIVE,cleanup,arch
- status: open
- behavior: The system attempts graceful shutdown via `SendCtrlAltDel` first on x86_64 hosts, escalating to forced termination only after the graceful window times out.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `stop_strategy_for_arch` lines 1297-1306; `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §4.2 Bounded stop.
- captured-by: m80/docs/behaviors/cleanup/arch-sensitive-stop.md#x86-graceful + m80/firecracker/tests/cleanup/arch_sensitive_stop.rs::x86_attempts_graceful_first

### Leaf: Force-only stop on architectures without graceful support
- parent_var: $L2_15_2
- labels: $ACTIVE,cleanup,arch
- status: open
- behavior: The system skips graceful shutdown on non-x86_64 hosts and uses forced termination directly, recording the disposition as `ForcedKillUnsupportedGraceful`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `StopStrategy::ForceOnlyUnsupportedGraceful` line 1304; `StopDisposition::ForcedKillUnsupportedGraceful` line 1263.
- captured-by: m80/docs/behaviors/cleanup/arch-sensitive-stop.md#non-x86-force-only + m80/firecracker/tests/cleanup/arch_sensitive_stop.rs::aarch64_uses_force_only

### Leaf: Record stop disposition (Graceful, ForcedKillFallback, ForcedKillUnsupportedGraceful)
- parent_var: $L2_15_2
- labels: $ACTIVE,cleanup,arch
- status: open
- behavior: The system records exactly which stop path was used so the release gate can distinguish graceful exit from forced fallback from forced-only on unsupported arch.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `StopDisposition` lines 95-103, 1249-1263; `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §4.2.
- captured-by: m80/docs/behaviors/cleanup/arch-sensitive-stop.md#disposition-record + m80/firecracker/tests/cleanup/arch_sensitive_stop.rs::disposition_recorded_per_path

## L2-15.3 Idempotent teardown (parent_var: $L2_15_3)

### Leaf: Repeat teardown calls without error on already-cleaned VMs
- parent_var: $L2_15_3
- labels: $ACTIVE,cleanup,idempotent
- status: open
- behavior: The system tolerates repeated stop and delete invocations against the same VM id, succeeding even when sockets, taps, scratch images, or run dir are already gone.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `cleanup_local_vm_artifacts` lines 1341-1366; dossier `07-modules-essential-vs-hygiene.md` §lifecycle.rs.
- captured-by: m80/docs/behaviors/cleanup/idempotent-teardown.md#repeat-safe + m80/firecracker/tests/cleanup/idempotent_teardown.rs::repeat_calls_succeed

### Leaf: Remove only owned runtime residue during cleanup
- parent_var: $L2_15_3
- labels: $ACTIVE,cleanup,ownership
- status: open
- behavior: The system removes only API/vsock sockets, runtime rootfs clone, scratch image, jailer materialization, and the per-VM run directory keyed to the owning VM id, leaving foreign or unowned residue untouched.
- source: predecessor `docs/gates/stage-g-firecracker-teardown-placement-release-contract.md` §4.5 Residue cleanup.
- captured-by: m80/docs/behaviors/cleanup/idempotent-teardown.md#owned-residue-only + m80/firecracker/tests/cleanup/idempotent_teardown.rs::leaves_unowned_residue_alone

### Leaf: Run the same ownership-aware cleanup at startup scavenging as during normal teardown
- parent_var: $L2_15_3
- labels: $ACTIVE,cleanup,recovery
- status: open
- behavior: The system reuses the live-teardown cleanup primitives during `startup_scavenge` so orphaned run directories from a prior process exit are reaped through the same idempotent path.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `force_stop_and_cleanup`/`cleanup_local_vm_artifacts` lines 1308-1366; agent reference §R6.
- captured-by: m80/docs/behaviors/cleanup/idempotent-teardown.md#startup-scavenge-reuse + m80/firecracker/tests/cleanup/idempotent_teardown.rs::startup_scavenge_uses_same_path

## L2-15.4 Force-kill preservation (parent_var: $L2_15_4)

### Leaf: Preserve run-dir for offline triage when force-stop-and-cleanup fails
- parent_var: $L2_15_4
- labels: $ACTIVE,cleanup,triage
- status: open
- behavior: The system retains the per-VM run directory after `force_stop_and_cleanup` when any cleanup step errored or when `preserve_run_dir` is set, so an operator can inspect residue offline.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` `force_stop_and_cleanup` and `remove_run_dir_after_cleanup` lines 1308-1381.
- captured-by: m80/docs/behaviors/cleanup/force-kill-preservation.md#run-dir-preserved + m80/firecracker/tests/cleanup/force_kill_preservation.rs::run_dir_kept_when_cleanup_fails

### Leaf: Use force_stop_and_cleanup as the last-resort teardown path
- parent_var: $L2_15_4
- labels: $ACTIVE,cleanup,triage
- status: open
- behavior: The system reaches `force_stop_and_cleanup` only as the final fallback, after graceful stop has been attempted (where supported) and bounded-stop has not produced a clean exit.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs` lines 1308-1339 (kill + cleanup-jailer + cleanup-network + cleanup-storage merge sequence).
- captured-by: m80/docs/behaviors/cleanup/force-kill-preservation.md#last-resort-only + m80/firecracker/tests/cleanup/force_kill_preservation.rs::only_invoked_after_bounded_stop

---

## L1-16 Snapshot / Restore

## L2-16.1 Manifest schemas (parent_var: $L2_16_1)

### Leaf: Reserve snapshot-manifest.json and restore-metadata.json as run-root file names
- parent_var: $L2_16_1
- labels: $ACTIVE,snapshot,schema
- status: open
- behavior: The system reserves the names `snapshot-manifest.json` and `restore-metadata.json` in the run root regardless of whether snapshot execution is wired in this version.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/snapshot.rs` `SNAPSHOT_MANIFEST_FILE` line 13, `RESTORE_METADATA_FILE` line 14; `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Current Boundary.
- captured-by: m80/docs/behaviors/snapshot/manifest-schema.md#reserved-names + m80/firecracker/tests/snapshot/manifest_schema.rs::reserved_names_round_trip

### Leaf: Validate FirecrackerSnapshotManifest fields and 5-element required artifact set
- parent_var: $L2_16_1
- labels: $ACTIVE,snapshot,schema
- status: open
- behavior: The system rejects any snapshot manifest that omits VmState, Memory, RuntimeRootfs, WorkspaceScratch, or VerifiedBootIdentity, and requires non-empty workspace_id/run_id/source_vm_id, an absolute source_run_dir, and a non-zero `created_at_unix_ms`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/snapshot.rs` `FirecrackerSnapshotManifest` lines 55-63 + `validate` lines 118-175; `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Contract rule 5.
- captured-by: m80/docs/behaviors/snapshot/manifest-schema.md#manifest-validation + m80/firecracker/tests/snapshot/manifest_schema.rs::manifest_requires_5_artifact_set

### Leaf: Validate FirecrackerRestoreMetadata schema with planned/materialized/failed phase
- parent_var: $L2_16_1
- labels: $ACTIVE,snapshot,schema
- status: open
- behavior: The system carries restore metadata as a typed schema with `schema_version`, source/restored vm_id pair, source/restored run dirs, manifest path, artifact set sha256, and a `FirecrackerRestorePhase` of Planned, Materialized, or Failed.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/snapshot.rs` `FirecrackerRestoreMetadata` lines 73-86 + `FirecrackerRestorePhase` lines 65-70.
- captured-by: m80/docs/behaviors/snapshot/manifest-schema.md#restore-metadata + m80/firecracker/tests/snapshot/manifest_schema.rs::restore_metadata_schema_round_trip

## L2-16.2 Persistence layout (parent_var: $L2_16_2)

### Leaf: Persist snapshots under <store-root>/<workspace_id>/<run_id>/<unix_ms>-<sha>/
- parent_var: $L2_16_2
- labels: $ACTIVE,snapshot,persistence
- status: open
- behavior: The system lays out persisted snapshot sets at `<store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/`, with the store root required to be an absolute host path.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md` §Contract rule 4 (deterministic backend-local layout).
- captured-by: m80/docs/behaviors/snapshot/persistence-layout.md#path-template + m80/firecracker/tests/snapshot/persistence_layout.rs::layout_uses_documented_template

### Leaf: Fail closed when the persistence destination already exists
- parent_var: $L2_16_2
- labels: $ACTIVE,snapshot,persistence
- status: open
- behavior: The system refuses to overwrite an existing persisted snapshot directory and returns an explicit collision error rather than replacing artifacts in place.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md` §Contract rule 7.
- captured-by: m80/docs/behaviors/snapshot/persistence-layout.md#collision-fail-closed + m80/firecracker/tests/snapshot/persistence_layout.rs::collision_fails_closed

### Leaf: Restrict the first-line persistence backend to host-local filesystem only
- parent_var: $L2_16_2
- labels: $ACTIVE,snapshot,persistence
- status: open
- behavior: The system supports only host-local filesystem persistence in v0.1 and explicitly does not introduce S3, GCS, Azure, `object_store`, or any generic storage trait.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-persistence-contract.md` §Contract rule 1; dossier `00-verdict.md` §Why this works #4.
- captured-by: m80/docs/behaviors/snapshot/persistence-layout.md#host-local-only + m80/firecracker/tests/snapshot/persistence_layout.rs::no_remote_store_seam

## L2-16.3 Execution lane (parent_var: $L2_16_3)

### Leaf: Pause/quiesce the VM before snapshot artifact capture (v0.2)
- parent_var: $L2_16_3
- labels: $DEFERRED_V02,snapshot,execution
- status: deferred
- behavior: The system pauses the running VM before capturing snapshot artifacts so the on-disk vmstate and memory are consistent at one quiesce point.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Contract rule 6 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/snapshot/execution-lane.md#pause-then-capture (v0.2) + m80/firecracker/tests/snapshot/execution_lane.rs::v0_2_pause_then_capture (v0.2)

### Leaf: Restore into a fresh vm_id and fresh run_dir, never in-place (v0.2)
- parent_var: $L2_16_3
- labels: $DEFERRED_V02,snapshot,execution
- status: deferred
- behavior: The system materializes a restored VM into a new run dir with a `restored_vm_id` distinct from `source_vm_id`; restore is never in-place resume of the source run directory.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Contract rule 4 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/snapshot/execution-lane.md#fresh-identity (v0.2) + m80/firecracker/tests/snapshot/execution_lane.rs::v0_2_fresh_identity (v0.2)

## L2-16.4 First-line VM sizing (parent_var: $L2_16_4)

### Leaf: Measure snapshot timing on 1 vCPU / 1024 MiB first-line VM shape (v0.2)
- parent_var: $L2_16_4
- labels: $DEFERRED_V02,snapshot,sizing
- status: deferred
- behavior: The system pegs snapshot timing proofs to the tracked first-line Firecracker shape of 1 vCPU and 1024 MiB rather than a benchmark-only configuration.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Contract rule 11 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/snapshot/first-line-sizing.md#1-vcpu-1024-mib (v0.2) + m80/firecracker/tests/snapshot/first_line_sizing.rs::v0_2_first_line_shape (v0.2)

### Leaf: Restrict the first execution slice to direct/no-jailer launch (v0.2)
- parent_var: $L2_16_4
- labels: $DEFERRED_V02,snapshot,sizing
- status: deferred
- behavior: The system targets only the direct (no-jailer) launch path in the first snapshot/restore execution slice; jailed snapshot file materialization is a later follow-on.
- source: predecessor `docs/gates/stage-g-firecracker-snapshot-restore-contract.md` §Contract rule 2 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/snapshot/first-line-sizing.md#direct-only (v0.2) + m80/firecracker/tests/snapshot/first_line_sizing.rs::v0_2_direct_only (v0.2)

---

## L1-17 Warm-pool / Blank VM

## L2-17.1 Reset evidence vocabulary (parent_var: $L2_17_1)

### Leaf: Define BlankVmResetDecision::Reusable | Discard as the only reset outcome (v0.2)
- parent_var: $L2_17_1
- labels: $DEFERRED_DEAD,warm-pool,vocabulary
- status: deferred
- behavior: The system models a blank-VM reset outcome as exactly two variants, `Reusable` carrying full proof or `Discard` carrying a discard reason; no third "maybe" or "retry" branch exists.
- source: predecessor `docs/gates/stage-g-blank-vm-reset-contract.md` §2 Contract Summary; predecessor `crates/sandbox/agent-sandbox-firecracker/src/blank_pool.rs` `BlankVmReusableEvidence` line 122 + `BlankVmDiscardEvidence` line 270 (vocabulary deferred to v0.2 wiring).
- captured-by: m80/docs/behaviors/warm-pool/reset-vocabulary.md#two-outcome-decision (v0.2) + m80/firecracker/tests/warm_pool/reset_vocabulary.rs::v0_2_two_outcome_decision (v0.2)

### Leaf: Cover 8 reset evidence inputs (ownership, boot identity, no-workspace-id, no-run-id, empty workspace, clean run-root, clean diagnostics, post-reset probe) (v0.2)
- parent_var: $L2_17_1
- labels: $DEFERRED_DEAD,warm-pool,vocabulary
- status: deferred
- behavior: The system requires `BlankVmResetProof` to span exactly the eight named inputs `CandidateOwnership`, `BootIdentity`, `WorkspaceIdentityAbsent`, `RunIdentityAbsent`, `GuestWorkspaceClean`, `HostRunRootClean`, `DiagnosticsClean`, and `GuestControlProbe`.
- source: predecessor `docs/gates/stage-g-blank-vm-reset-contract.md` §3 Reset Inputs; predecessor `crates/sandbox/agent-sandbox-firecracker/src/blank_pool.rs` `BlankVmResetRequirement` lines 33-57 + `RESET_REQUIREMENTS` lines 59-68.
- captured-by: m80/docs/behaviors/warm-pool/reset-vocabulary.md#8-input-evidence (v0.2) + m80/firecracker/tests/warm_pool/reset_vocabulary.rs::v0_2_eight_input_evidence (v0.2)

### Leaf: Enumerate BlankVmResetDiscardReason for every Discard outcome (v0.2)
- parent_var: $L2_17_1
- labels: $DEFERRED_DEAD,warm-pool,vocabulary
- status: deferred
- behavior: The system carries every `Discard` outcome with one of the documented `BlankVmResetDiscardReason` values (e.g., `MissingOwnership`, `BootIdentityMismatch`, `GuestWorkspaceDirty`, `AmbiguousResidue`) so downstream consumers can reason about the cause.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/blank_pool.rs` `BlankVmResetDiscardReason` lines 227-267 (vocabulary deferred to v0.2 wiring).
- captured-by: m80/docs/behaviors/warm-pool/reset-vocabulary.md#discard-reasons (v0.2) + m80/firecracker/tests/warm_pool/reset_vocabulary.rs::v0_2_discard_reasons_enumerated (v0.2)

## L2-17.2 Non-inference rule (parent_var: $L2_17_2)

### Leaf: Forbid inferring reuse from liveness, handles, sockets, metrics, or clean-looking trees (v0.2)
- parent_var: $L2_17_2
- labels: $DEFERRED_DEAD,warm-pool,non-inference
- status: deferred
- behavior: The system MUST NOT classify a candidate as `Reusable` from process liveness, open file handles, socket existence, metrics presence, or a directory tree that merely looks clean; only fresh, explicit, backend-local proof of all 8 inputs justifies reuse.
- source: predecessor `docs/gates/stage-g-blank-vm-reset-contract.md` §2 Contract Summary final paragraph (deferred to v0.2 wiring).
- captured-by: m80/docs/behaviors/warm-pool/non-inference.md#forbidden-inference-sources (v0.2) + m80/firecracker/tests/warm_pool/non_inference.rs::v0_2_inference_sources_rejected (v0.2)

### Leaf: Reject any missing, stale, ambiguous, or inferred input by deleting and recreating (v0.2)
- parent_var: $L2_17_2
- labels: $DEFERRED_DEAD,warm-pool,non-inference
- status: deferred
- behavior: The system collapses to delete-and-recreate whenever any reset evidence input is missing, stale, ambiguous, or inferred; there is no compatibility path that treats older layouts or partial evidence as reusable.
- source: predecessor `docs/gates/stage-g-blank-vm-reset-contract.md` §2 Contract Summary, §7 No Compatibility Heuristics (deferred to v0.2 wiring).
- captured-by: m80/docs/behaviors/warm-pool/non-inference.md#delete-and-recreate-on-doubt (v0.2) + m80/firecracker/tests/warm_pool/non_inference.rs::v0_2_delete_and_recreate_default (v0.2)

---

## L1-18 Observability Tail

## L2-18.1 Diagnostics event log (parent_var: $L2_18_1)

### Leaf: Write VM-lifecycle events to <run_dir>/diagnostics.jsonl with schema_version 2 (v0.2)
- parent_var: $L2_18_1
- labels: $DEFERRED_V02,observability,diagnostics
- status: deferred
- behavior: The system writes one JSON object per line to `<run_dir>/diagnostics.jsonl`, each event tagged with `schema_version`, timestamp, source class, phase, message, and bounded context fields.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/diagnostics.rs` `DIAGNOSTICS_SCHEMA_VERSION` line 13 + `DiagnosticEvent` lines 74-86 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/diagnostics-log.md#jsonl-format (v0.2) + m80/firecracker/tests/observability/diagnostics_log.rs::v0_2_jsonl_format (v0.2)

### Leaf: Cover the documented diagnostic phases (StartupScavenge through Delete) (v0.2)
- parent_var: $L2_18_1
- labels: $DEFERRED_V02,observability,diagnostics
- status: deferred
- behavior: The system tags each diagnostic event with one of the named phases `StartupScavenge`, `HostPreflight`, `StoragePrepare`, `NetworkPrepare`, `Boot`, `Ready`, `Request`, `Stop`, `Writeback`, or `Delete`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/diagnostics.rs` `DiagnosticPhase` lines 39-50 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/diagnostics-log.md#phase-enum (v0.2) + m80/firecracker/tests/observability/diagnostics_log.rs::v0_2_phase_enum_complete (v0.2)

### Leaf: Wrap diagnostics in Option<VmDiagnostics> so removal does not break boot (v0.2)
- parent_var: $L2_18_1
- labels: $DEFERRED_V02,observability,diagnostics
- status: deferred
- behavior: The system carries the diagnostics handle as `Option<VmDiagnostics>` on the running VM so disabling or failing to materialize the diagnostics writer never blocks boot or teardown.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/diagnostics.rs` `VmDiagnostics` lines 143-149; dossier `07-modules-essential-vs-hygiene.md` §diagnostics.rs (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/diagnostics-log.md#option-wrapper (v0.2) + m80/firecracker/tests/observability/diagnostics_log.rs::v0_2_option_wrapper_safe (v0.2)

## L2-18.2 Per-VM probe (parent_var: $L2_18_2)

### Leaf: Walk run_root and emit FirecrackerVmProbeRecord for each owned run dir (v0.2)
- parent_var: $L2_18_2
- labels: $DEFERRED_V02,observability,probe
- status: deferred
- behavior: The system enumerates `<run_root>/*/`, skipping unowned dirs, and emits one `FirecrackerVmProbeRecord` per owned VM with vm_id, run_dir, health, lifecycle phase, and reachability flags.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/probe.rs` `collect_probe_snapshot` lines 84-122 + `FirecrackerVmProbeRecord` lines 51-63 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/probe.md#run-root-walk (v0.2) + m80/firecracker/tests/observability/probe.rs::v0_2_run_root_walk (v0.2)

### Leaf: Classify probe health from host-visible truth only (v0.2)
- parent_var: $L2_18_2
- labels: $DEFERRED_V02,observability,probe
- status: deferred
- behavior: The system derives `FirecrackerVmHealth` (Healthy, Degraded, Stuck, Exited) from ownership markers, lease liveness, and socket reachability rather than from logs or metrics presence.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/probe.rs` `derive_probe_health` invocation lines 137-144 + `FirecrackerVmHealth` lines 43-48 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/probe.md#host-visible-only (v0.2) + m80/firecracker/tests/observability/probe.rs::v0_2_host_visible_only (v0.2)

## L2-18.3 Health/readiness aggregation (parent_var: $L2_18_3)

### Leaf: Aggregate probe records into FirecrackerHealthSnapshot with rollout readiness (v0.2)
- parent_var: $L2_18_3
- labels: $DEFERRED_V02,observability,health
- status: deferred
- behavior: The system summarizes per-VM probe records into a `FirecrackerHealthSnapshot` carrying a rollout-readiness summary plus per-workspace health views.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs` `FirecrackerHealthSnapshot` lines 14-19 + `collect_health_snapshot` lines 21-32 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/health.md#snapshot-shape (v0.2) + m80/firecracker/tests/observability/health.rs::v0_2_snapshot_shape (v0.2)

### Leaf: Render health snapshot as JSON via render_health_snapshot (v0.2)
- parent_var: $L2_18_3
- labels: $DEFERRED_V02,observability,health
- status: deferred
- behavior: The system renders the aggregated health snapshot as pretty JSON via `render_health_snapshot(run_root)`, returning a string the caller can ship or write to disk.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs` `render_health_snapshot` lines 34-42 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/health.md#json-render (v0.2) + m80/firecracker/tests/observability/health.rs::v0_2_json_render (v0.2)

## L2-18.4 Prometheus scrape (parent_var: $L2_18_4)

### Leaf: Render Prometheus text via render_prometheus_metrics (rendering only, no HTTP server) (v0.2)
- parent_var: $L2_18_4
- labels: $DEFERRED_V02,observability,scrape
- status: deferred
- behavior: The system renders Prometheus exposition-format text from the run_root via `render_prometheus_metrics` and exposes only the rendering helper, leaving HTTP serving to the embedding application.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/scrape.rs` `render_prometheus_metrics` lines 44-154 (deferred to v0.2 implementation).
- captured-by: m80/docs/behaviors/observability/prometheus.md#render-only (v0.2) + m80/firecracker/tests/observability/prometheus.rs::v0_2_render_only (v0.2)
