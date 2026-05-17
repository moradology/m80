# Migrating To Pmem And Snapshot Restore

Bead: `m80-q420k.5.7`.

This guide shows a staged migration for existing m80 consumers. Keep the
migration split: first add read-only pmem layers without changing warm
strategy, then move from boot-fill warm slots to snapshot-template restore.

The examples use the planned BootSpec shape from `docs/examples/`. Parser and
CLI hardening are separate Phase E leaves.

## Stage 1: Add Pmem Layers Only

Start by moving large read-only artifacts, such as toolchains, into erofs pmem
layers while keeping the existing warm strategy.

Before:

```yaml
rootfs:
  base: "sha256:rootfs-base"
warm_strategy:
  boot_fill: {}
```

After:

```yaml
rootfs:
  base: "sha256:rootfs-base"
pmem_layers:
  - image: "sha256:toolchain-erofs"
    sharing:
      per_vm: {}
    mount_at: "/opt/m80-layers/toolchain"
warm_strategy:
  boot_fill: {}
```

Operator flow:

1. Build or import the erofs image with `m80 image build`.
2. Verify it with `m80 image verify sha256:toolchain-erofs`.
3. Deploy the BootSpec with `PmemSharing::PerVm`.
4. Run real-KVM smoke for the consumer workload.
5. Watch `m80_pmem_layers_per_vm_count{sharing="per_vm"}` and guest DAX mount
   diagnostics.

This stage should not change restore behavior, template stores, or hook
execution. Rollback is to remove `pmem_layers` and redeploy the old BootSpec.

## Stage 2: Consider Shared Pmem

Switch from `PerVm` to `Shared` only for same-trust-domain guests. Shared is a
capacity optimization, not an isolation feature. The side channel is cache
timing via shared DAX-backed pages, and the assumption is documented in
`docs/positioning.md` "Trust-Domain Assumption for Shared Pmem".

Before:

```yaml
pmem_layers:
  - image: "sha256:toolchain-erofs"
    sharing:
      per_vm: {}
    mount_at: "/opt/m80-layers/toolchain"
```

After:

```yaml
pmem_layers:
  - image: "sha256:toolchain-erofs"
    sharing:
      shared:
        trust_reason: "same_operator"
    mount_at: "/opt/m80-layers/toolchain"
```

Operator flow:

1. Confirm the guests sharing the layer are inside one trust domain.
2. Record the reason as the typed trust witness, not a free-form note.
3. Verify the image with `m80 image verify`.
4. Use `m80 image show` and `m80 warm status` to confirm the expected
   image and active leases.
5. Wait for the verified density artifact before treating memory savings as a
   production claim.

Rollback is to switch the same image digest back to `PerVm`. Do not delete the
canonical image-store artifact as part of rollback.

## Stage 3: BootFill To SnapshotRestore

After pmem layers are stable, move warm slots from boot-fill to
snapshot-template restore.

Before:

```yaml
warm_strategy:
  boot_fill: {}
```

After:

```yaml
warm_strategy:
  snapshot_restore:
    template: "sha256:template-fingerprint"
    hooks:
      - reseed_systemd_random_seed: {}
      - regen_machine_id: {}
      - set_hostname:
          value: "m80-template"
```

Operator flow:

1. Build a template with `m80 template build`.
2. Inspect it with `m80 template show sha256:template-fingerprint`.
3. Start or restart the explicit warm owner with `m80 warm enable`.
4. Use `m80 warm status` and Prometheus `m80_restore_latency_seconds` to check
   the restore path.
5. Use `m80 logs <vm-id>` when a post-restore hook fails; hook failure rejects
   the lease and tears the VM down.

Rollback is to set `warm_strategy` back to `boot_fill` and drain the warm owner:

```sh
m80 warm drain
m80 warm disable
```

Then redeploy the BootSpec with `boot_fill` and start the owner again if needed.

## Upgrade Compatibility Windows

Snapshot templates are tied to fingerprint inputs:

- host kernel version;
- Firecracker version;
- guest kernel digest;
- pmem image digest set;
- post-init state digest;
- hook-spec-set digest.

During a host kernel, Firecracker, or guest kernel rollout, expect old
templates to become invalidated. This is intentional. Do not build
compatibility shims that restore stale templates across incompatible inputs.

Recommended window:

1. Keep the current warm owner running while new hosts are prepared.
2. Stop new lease admission or drain the owner.
3. Upgrade the host/Firecracker/guest artifacts.
4. Run `m80 preflight`.
5. Use `m80 template prune --boot-spec <file>` to remove unpinned templates in
   the same conservative family scope whose fingerprint no longer matches the
   current host/kernel/Firecracker tuple. Inspect anything outside that scope
   with `m80 template list` and `m80 template show <fingerprint>`, then remove
   known-stale templates with `m80 template rm <fingerprint>`.
6. Build new templates and restart warm owners.

If the rollout fails, roll back host artifacts and use `boot_fill` until a new
template can be built and measured.

## Host Kernel Bump Cleanup

After a host kernel bump, template fingerprints should change. Use this flow:

```sh
m80 warm drain
m80 preflight
m80 template list
m80 template prune --boot-spec ./boot-spec.yaml
m80 template build <name> --boot-spec ./boot-spec.yaml
m80 warm enable --foreground --size 4
```

Do not close measurement-shaped beads from this operator flow unless the run
also writes the required committed real-substrate artifact.

## Verification Pointers

- Pmem layer behavior: `docs/behaviors/rootfs/pmem-layers.md`.
- Shared pmem behavior: `docs/behaviors/rootfs/pmem-shared.md`.
- Template lifecycle: `docs/decisions/0007-snapshot-template-lifecycle.md`.
- Post-restore hooks: `docs/behaviors/warm-pool/post-restore-hooks.md`.
- Restore latency measurement: `docs/perf/snapshot-template-restore.md`.
- Composed e2e measurement: future `docs/perf/composed-e2e.md`.
