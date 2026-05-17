# Reflink Rootfs Smoke

The rootfs overlay divergence smoke boots a real Firecracker VM, writes 16 MiB
of random data inside the guest root filesystem, stops the VM, and checks the
host-side overlay allocation grew while the run-root-local empty overlay
template did not.

Run:

```sh
M80_VERIFY_REFLINK_DIVERGENCE=1 ./scripts/smoke.sh launch-only
```

The smoke prints:

- `overlay_blocks_before` / `overlay_blocks_after`
- `template_blocks_before` / `template_blocks_after`
- `overlay_filefrag_*` and `template_filefrag_*` snapshots
- `REFLINK_DIVERGENCE_OK` on success

The check is valid on both reflink and byte-copy hosts. Reflink-capable hosts
should start with a low overlay allocation because the overlay is a metadata
clone of the empty template. Non-reflink hosts still pass because guest writes
must grow the per-VM overlay without changing the template.
