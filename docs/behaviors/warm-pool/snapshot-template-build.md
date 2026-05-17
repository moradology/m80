# Snapshot Template Build

Snapshot-template builds are the producer side of Phase D warm pools. The
build path cold-boots a stateless VM, waits for normal guest readiness through
`Sandbox::launch`, captures the VM under
`<run_root>/.template-capture/<fingerprint>-<pid>/`, then publishes the
captured body into `m80-snapshot-template` staging before committing it into
the content-addressed store. Capture stays inside run-root scope; the committed
template store does not have to.

## Post-init identity

The producer computes `PostInitDigest` from typed observables instead of prose
or ad hoc JSON. The current v1 observable set includes:

- m80 wire protocol version
- image kind and rootfs format from preflight's validated image manifest
- guestd sha256 and rootfs sha256 from the same manifest
- declared vCPU count, memory size, CPU template, and caller boot-arg extras
- declared pmem layer identity: guest mount path, image digest, sharing mode,
  and jail-visible backing path

Those observables become `TemplateInputs::post_init_state_digest`; the ordered
`HookSpecSet` contributes through its canonical hook-set digest. A caller-supplied
`TemplateInputs` that does not match current host discovery and the declared
`SandboxConfig` fails before launch.

## Store commit

The snapshot body is invisible while it lives under `staging/`. A captured but
uncommitted body is still a cache miss, and dropping the build plan publishes
nothing. `TemplateStore::commit` writes `manifest.json` and the restore-side
`snapshot-manifest.json` only after both `vm.snap` and `mem.snap` exist, then
renames the staging directory into `by-fingerprint/<hex>/`. The `m80-snapshot`
manifest records the final content-addressed artifact paths, not temporary
staging paths.

## Warm-pool fill

`WarmStrategy::SnapshotRestore` carries the `TemplateStore`, ordered
`HookSpecSet`, and ready-probe request used by the fill worker. For each slot,
the worker computes live `TemplateInputs` from current preflight discovery,
the stateless sandbox config, and the hook set. It then asks the producer path
to look up or build that exact fingerprint. Cache hit returns a process-local
`PinnedTemplate`; cache miss is explicit build work. A committed template whose
manifest no longer matches the live inputs returns a typed fingerprint mismatch
and records a fill failure. The worker does not silently rebuild on lease
checkout, downgrade to `DirectSnapshot`, or cold-launch a ready slot through a
different strategy.

After a template is pinned, the worker restores the committed body through the
template-body bind path, runs the post-restore hook gate, runs the configured
ready probe, and only then pushes the slot into the ready queue.

## Evidence

- `crates/m80-firecracker/src/warm_pool/template_build.rs` owns the producer
  path and the lookup/build primitive used by `WarmStrategy::SnapshotRestore`.
- `crates/m80-firecracker/src/warm_pool/template_build/tests.rs` covers
  post-init digest stability, fingerprint invalidation, restore-layout metadata,
  fail-closed input mismatch, cache-miss/cache-hit behavior, and ignored
  real-KVM template-build plus SnapshotRestore fill scenarios.
- `crates/m80-firecracker/src/warm_pool/tests.rs` covers fill-counter behavior
  for a tampered committed template manifest.
- `crates/m80-snapshot-template/tests/store.rs` covers staging invisibility and
  final artifact paths in the embedded snapshot manifest.
