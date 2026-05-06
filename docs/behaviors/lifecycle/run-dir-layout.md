# Run-Directory Layout

## Per-VM Dir

m80 creates each VM's run directory at `<run_root>/<vm_id>/`.
`m80-firecracker` computes that path with `run_dir_path(run_root, vm_id)` and
phase 1 of `Sandbox::launch` creates it before writing VM-local state beneath
it. This is the m80 form of the predecessor behavior captured in
`crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:367-425`
(`VmPaths::new`, `run_root.join(vm_id)`).

The parent run-root is admitted by preflight first. Creating the per-VM
directory is not stale-state recovery and does not mean m80 silently repairs a
missing or unusable host run-root.

## API Socket

m80's Firecracker REST API socket is
`<run_root>/<vm_id>/<firecracker basename>/<vm_id>/root/firecracker.sock`.
The socket is inside the jailer chroot because m80 delegates chroot layout to
Firecracker's jailer. This deliberately differs from the older predecessor path
`<run_dir>/firecracker.sock` recorded at
`crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:409`.

`m80-firecracker` computes the host-visible API socket with
`firecracker_api_socket_path(run_dir, firecracker_bin)` and uses that same path
for jailer launch and REST client connection.

## Vsock Socket

m80's host-side vsock muxer socket is
`<run_root>/<vm_id>/<firecracker basename>/<vm_id>/root/vsock.sock`.
As with the API socket, the path lives inside the jailer chroot rather than
directly under `<run_root>/<vm_id>/`. This is the m80 form of the older predecessor
behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:410`.

`m80-firecracker` computes this path with
`vsock_socket_path(run_dir, firecracker_bin)` and uses it for ready probing,
exec, PTY, cancellation, and stop RPCs.

## Image Paths

m80 keeps the shared base rootfs read-only and does not copy it into each run
directory. The per-VM writable rootfs overlay is
`<run_root>/<vm_id>/rootfs.overlay.ext4`, and the optional workspace scratch
image is `<run_root>/<vm_id>/scratch.ext4`.

This differs from the older predecessor behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:413-414`, where a
runtime rootfs clone and workspace scratch image were co-located beside the
sockets. In m80 the writable images remain co-located in the per-VM run
directory, while the sockets move into the jailer chroot.

## Reaped On Delete

After a VM is stopped, `StoppedSandbox::delete` removes the entire per-VM run
directory with `remove_dir_all`. `StoppedSandbox::preserve_for_triage` is the
explicit exception: it moves the run directory under `.preserved/` and then
releases the admission permit. This is the m80 form of the predecessor cleanup
behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1368-1381`.
