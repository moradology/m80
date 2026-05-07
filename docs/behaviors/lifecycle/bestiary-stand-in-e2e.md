# Bestiary Stand-In E2E

Captured by bead `m80-iswt.9`.

m80 proves the conveyor-belt primitive set with an ignored real-KVM integration
test that stands in for the external Bestiary allocator. The test creates a
generic snapshot-backed warm pool, leases a one-shot slot, creates a tenant
ext4 drive image visible inside the Firecracker jail, attaches that image
through preallocated hotplug slot 0, verifies opaque tenant-identity bytes
through guestd, runs one workload, destroys the one-shot VM, waits for pool
refill, and checks that the replacement slot has no tenant mount residue.

The warm-pool lease is the caller facade for this path:
`WarmLease::attach_drive_verified` delegates to the underlying
`RunningSandbox::attach_drive_verified` before the first workload exec. A
successful attach leaves the one-shot token unconsumed. An attach failure
means the running slot is not reusable; the lease releases the slot and starts
background refill.

The real-KVM proof is ignored by default because it needs `/dev/kvm`, root
or equivalent privileges, Firecracker and jailer binaries, snapshot support,
the m80 guest image, and `mkfs.ext4`. Run it explicitly on a prepared host:

```sh
sudo cargo test -p m80-firecracker -- --ignored bestiary_stand_in
```

Evidence:

- `crates/m80-firecracker/src/warm_pool/lease.rs` exposes
  `WarmLease::attach_drive_verified` for pre-workload tenant drive attach.
- `crates/m80-firecracker/tests/bestiary_stand_in_real_kvm.rs` contains the
  ignored `bestiary_stand_in_attach_identity_run_destroy_no_residue`
  proof.
- `crates/m80-firecracker/README.md` documents the warm-lease attach facade
  and the KVM-gated Bestiary stand-in test.
