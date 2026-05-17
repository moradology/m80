# Shared Pmem Layers

Bead: `m80-q420k.3.6`

`PmemSharing::Shared(TrustDomainAck)` is a same-trust-domain optimization for
read-only erofs layers. It deliberately points multiple VMs at one canonical
`m80-image-store` inode so the host page cache can be shared across those VMs.
It is not a cross-tenant isolation primitive; see the trust-domain paragraph in
`docs/positioning.md`.

## declaration

Callers must construct the typed witness explicitly:

```rust
use m80_firecracker::{
    ErofsImageRef, GuestMountPath, ImageDigest, PmemLayer, PmemSharing,
    TrustDomainAck, TrustReason,
};

let digest = ImageDigest::parse(
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
)?;
let layer = PmemLayer::new(
    ErofsImageRef::from_digest(digest),
    PmemSharing::Shared(TrustDomainAck::new(TrustReason::SameOperator)),
    GuestMountPath::parse("/opt/m80-layers/toolchain")?,
);
# Ok::<_, Box<dyn std::error::Error>>(())
```

There is no `Default` for `TrustDomainAck`, and `PmemSharing::Shared` accepts
no caller-provided writability hint.

## e2e-scenarios

1. Inode identity: two VMs declaring `Shared` for the same digest bind the same
   canonical image-store artifact into their jails. The real-KVM test compares
   host `(st_dev, st_ino)` for both jail paths against the image-store path.
2. Host page sharing: the expected density win is that N guests fault the same
   DAX-backed erofs inode, so host memory grows like one shared artifact plus
   per-VM overhead. The measurement gate lives in `m80-q420k.3.8`.
   `docs/perf/erofs-dax-sharing-layout.md` proves that this claim only applies
   to uncompressed non-inlined erofs files on the current substrate. Compressed
   erofs files can still mount with `dax=always`, but guest `statx` does not
   report `STATX_ATTR_DAX` for those files, so they are not valid inputs for
   the Shared density proof. Storage prep rejects compressed Shared erofs
   artifacts before active-use marker creation, jail materialization, or
   Firecracker admission.
3. Compile-time safety: rustdoc `compile_fail` examples pin that `Shared`
   cannot be constructed without `TrustDomainAck`, and that the ack has no
   implicit default.
4. Mixed-mode coexistence: one VM can use `Shared` while another uses `PerVm`
   for the same digest. The Shared VM uses the store inode; the PerVm VM gets
   a run-dir clone.
5. Active-use lifecycle: Shared storage prep creates
   `<store>/shared/<digest>/refs/<vm_id>`. Stop and force-kill release the
   marker after jail bind mounts are gone. Backend startup sweeps stale markers
   for VM ids no longer present under the run root. Marker release does not
   delete the canonical artifact.
6. Negative API case: arbitrary writability hints are rejected by type shape,
   not by runtime string parsing.

## references

- `docs/behaviors/rootfs/pmem-layers.md`
- `docs/behaviors/jailer/pmem-bindings.md`
- `docs/perf/erofs-dax-sharing-layout.md`
- `docs/decisions/0006-pmemlayer-api.md`
- `crates/m80-firecracker/tests/erofs_dax_layout_real_kvm.rs::erofs_dax_layout_matrix_real_kvm`
- `crates/m80-firecracker/tests/pmem_layer_real_kvm.rs::pmem_layer_shared_reuses_backing_inode_real_kvm`
- `crates/m80-image-store/tests/store.rs::shared_ref_acquire_and_release_updates_marker_count_without_deleting_artifact`
