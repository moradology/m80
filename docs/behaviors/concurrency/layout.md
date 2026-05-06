# Run-Root Layout Invariants

## Per VM Dir

All per-VM state owned by `m80-firecracker` starts under
`<run_root>/<vm_id>/`. The public helper `run_dir_path(run_root, vm_id)` pins
that path. Writable VM images, console log, diagnostics, boot identity, and the
ownership lock are direct children of that per-VM directory. Firecracker and
vsock sockets live below the jailer chroot nested under the same run-dir.

Test:
- `crates/m80-firecracker/tests/concurrency/layout.rs::state_lives_under_run_root_slash_vm_id`

## Sha256 Naming

Shared outbound-network names are derived from the run-root path digest in
`m80-net-outbound`. The bridge name is `brfc` plus digest bytes from
`sha256(run_root_path)`, and the TAP name is derived from
`sha256(run_root_path || vm_id)`. Two resident m80 processes that use distinct
run-roots therefore derive distinct bridge/TAP names for the same VM id.

This invariant belongs to `m80-net-outbound`; `m80-firecracker` only preserves
the run-root boundary that feeds it.

Test:
- `crates/m80-firecracker/tests/concurrency/layout.rs::derives_unique_names_per_run_root`
