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

The run directory also carries `jailer-state.json`. `firecracker_pid` is the
live Firecracker process. `jailer_pid` is either a live jailer parent process or
`0`, the documented no-live-jailer sentinel used after the parent has exited.

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

## Snapshot Staging

m80 creates `<run_root>/<vm_id>/snapshot/` during launch and binds it into the
jail as `/snapshot` for snapshot capture staging. The directory is part of the
normal per-VM run-dir layout even before a caller invokes
`RunningSandbox::capture`; delete removes it with the rest of the run-dir.

## Snapshot Template Store

Snapshot-template bodies are content-addressed store artifacts, not per-VM
run-dir state. The template store layout is owned by
[`crates/m80-snapshot-template/README.md`](../../../crates/m80-snapshot-template/README.md):
committed bodies live under `by-fingerprint/<hex>/` with `vm.snap`, `mem.snap`,
`manifest.json`, and `snapshot-manifest.json`.

The template build path captures into
`<run_root>/.template-capture/<fingerprint>-<pid>/` first, then publishes
`vm.snap`, `mem.snap`, `manifest.json`, and `snapshot-manifest.json` into the
template store. This keeps Firecracker capture inside the run-root scope while
allowing the committed template store to live elsewhere.

## Template Body Bind

Template restore binds the committed template body directory read-only at
`/snapshot` using the existing jailer snapshot-parent plumbing. Before binding,
`m80-firecracker` rejects symlinked components in the template body paths and
canonicalizes the body parent. The restored VM sees the stable jail-visible
paths `/snapshot/vm.snap` and `/snapshot/mem.snap`; the host-visible store path
does not become per-VM mutable state and is not removed by
`StoppedSandbox::delete`.

The fill-side restore sequence is captured in
[`docs/behaviors/warm-pool/snapshot-restore-lifecycle.md`](../warm-pool/snapshot-restore-lifecycle.md).

## Post-Restore Hook Surface

Post-restore hooks do not add host-side run-dir artifacts. Hook execution is a
vsock RPC against the restored guest: the host sends `PostRestoreHookRequest`,
guestd mutates guest state as requested, and the host only records normal
diagnostics/failure-summary data if the restore path fails. The run directory
does not receive per-hook marker files, nonce files, or hook output payloads.

The hook contract is captured in
[`docs/behaviors/warm-pool/post-restore-hooks.md`](../warm-pool/post-restore-hooks.md).

## Launch Failure Summary

After phase 1 creates the run directory, launch and restore failures write
`<run_root>/<vm_id>/failure_summary.json` before cleanup guards run. The summary
contains `vm_id`, `failed_phase`, `error_variant`, `error_display`,
`timestamp_unix_ms`, and optional `request_id`.

By default, the launch-failure cleanup guard moves that partial run directory
under `<run_root>/.preserved/<unix_ms>-<vm_id>/` instead of deleting it, so the
summary, `diagnostics.jsonl`, and `console.log` remain available for triage.
`Sandbox::delete_run_dir_on_launch_error()` is the explicit opt-in to the old
delete-on-failure behavior.

## Reaped On Delete

After a VM is stopped, `StoppedSandbox::delete` removes the entire per-VM run
directory with `remove_dir_all`. `StoppedSandbox::preserve_for_triage` is the
explicit exception: it moves the run directory under `.preserved/` and then
releases the admission permit. This is the m80 form of the predecessor cleanup
behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1368-1381`.
