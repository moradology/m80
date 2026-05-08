# Image Build — Write Atomicity Under Signal

Source: `crates/m80-image-build/src/pipeline.rs` and
`crates/m80-image-build/src/minimal.rs`.

## loop-mount namespace {#loop-mount-namespace}

Before either image kind loop-mounts the output rootfs, the builder enters a
new mount namespace with `CLONE_NEWNS` and makes `/` recursively private with
`MS_REC | MS_PRIVATE`. The loop mount is therefore visible to the builder and
its child commands only; it is not propagated into the host mount namespace.

If the builder is killed while the rootfs is mounted, process death drops the
private mount namespace and the host should not retain a stale loop mount.

Test:
`m80-image-build/tests/write_atomicity_under_signal.rs::image_build_write_atomicity_under_signal`
(ignored; requires `CAP_SYS_ADMIN` or root).

## manifest-after-success {#manifest-after-success}

`<output_rootfs>.manifest.json` is written only after the mutable rootfs phase
has completed, the loop mount has been unmounted, the host audit copy of
`m80-guestd` has been written, and all artifact hashes have been computed.
A signal during the loop-mount/install phase must therefore leave no partial
manifest beside the output rootfs.

The ignored signal regression runs the real `m80-image-build run` Minimal path
with a debug-only pause immediately after `loop_mount`, sends `SIGKILL`, and
verifies:

- `<output_rootfs>.manifest.json` does not exist.
- the loop mount path is absent from the parent process mount table.
- the output directory can be removed without manual cleanup.
