# Tracing Span Catalog

Bead: `m80-q420k.5.5`.

This document defines the canonical tracing span names for layered-rootfs and
snapshot-template VM mechanics. It is a catalog, not a subscriber or logging
policy. Emitters should use `m80_observability::spans::*` names and attach the
fields listed here when those values are available at the emit site.

m80 spans describe VM mechanics only. They must not grow adapter-level agent
semantics such as tool names, idempotency keys, workspace ids, or Stage-H
`sandbox_exec_*` events.

## Catalog

| Span | Purpose | Fields | Expected emit sites |
| --- | --- | --- | --- |
| `m80.image.build` | Host-side image artifact build. | `image_digest`, `image_kind`, `source_digest`, `output_bytes`, `duration_us` | `crates/m80-image-build`; future `m80-cli image build` surface |
| `m80.pmem.attach` | Host-side Firecracker pmem device attach. | `vm_id`, `slot`, `image_digest`, `sharing_mode`, `backing_path`, `mount_path` | `crates/m80-firecracker/src/lifecycle/exec.rs`; m80-firecracker storage preparation |
| `m80.guest.dax_mount` | Guest-side erofs DAX mount of a pmem device. | `device`, `mount_path`, `fs_type`, `dax_mode`, `image_digest` | `crates/m80-guestd` pmem mount handler |
| `m80.template.build` | Snapshot-template body capture. | `template_fingerprint`, `template_body`, `pmem_layer_count`, `hook_spec_digest`, `duration_us` | `crates/m80-firecracker/src/warm_pool/template_build.rs` |
| `m80.template.restore` | Warm-pool slot restore from a snapshot-template body. | `template_fingerprint`, `vm_id`, `restore_nonce`, `snapshot_body`, `duration_us` | `crates/m80-firecracker/src/launch.rs`; `crates/m80-firecracker/src/warm_pool/fill_worker.rs` |
| `m80.post_restore_hook` | One closed typed post-restore hook execution. | `template_fingerprint`, `restore_nonce`, `hook_index`, `hook_variant`, `outcome`, `duration_us` | `crates/m80-guestd`; m80-firecracker post-restore hook request path |

## Field Schema

All fields are structured tracing fields. They are omitted when an emitter does
not have the value at that point in the lifecycle; they are not filled with
placeholder strings.

- `backing_path`: host path passed to Firecracker for a pmem backing.
- `dax_mode`: DAX option observed after mount, such as `dax=always`.
- `device`: guest block-device path, such as `/dev/pmem0`.
- `duration_us`: measured duration in microseconds for the span's primary
  operation.
- `fs_type`: mounted filesystem type. Current pmem layers use `erofs`.
- `hook_index`: zero-based index in the `HookSpecSet`.
- `hook_spec_digest`: digest of the typed post-restore hook set.
- `hook_variant`: closed `HookSpec` variant, such as
  `ReseedSystemdRandomSeed`, `RegenMachineId`, or `SetHostname`.
- `image_digest`: content digest of an image artifact or pmem layer.
- `image_kind`: image role or format, such as rootfs, erofs pmem layer, or
  snapshot template input.
- `mount_path`: admitted guest mount path.
- `outcome`: typed result, such as `ok` or the finite error variant.
- `output_bytes`: byte size of a produced image.
- `pmem_layer_count`: number of pmem layers included in a template.
- `restore_nonce`: opaque host-provided nonce used to pair restore and hook
  execution. It is not a semantic correlation id.
- `sharing_mode`: declared `PmemSharing` mode, such as `PerVm` or `Shared`.
- `slot`: zero-based pmem slot.
- `snapshot_body`: content-addressed snapshot body loaded by Firecracker.
- `source_digest`: digest of the source artifact set when available.
- `template_body`: content-addressed template body.
- `template_fingerprint`: stable fingerprint of template inputs.
- `vm_id`: opaque VM id for the Firecracker instance.

## Emitter Rules

- Use the constant names from `m80_observability::spans`.
- Add fields only when the emitting side actually owns the value.
- Keep caller semantic ids out of these spans. The only allowed correlation
  token here is an opaque `request_id` when an existing VM lifecycle request is
  already in scope.
- `m80.post_restore_hook` uses closed hook variants only. Arbitrary user shell
  commands are not part of the v0.1 hook surface.

## Verification

- `crates/m80-observability/tests/span_catalog.rs` asserts the catalog is
  non-empty, every named constant appears in `ALL_SPANS`, and all names are
  unique.
