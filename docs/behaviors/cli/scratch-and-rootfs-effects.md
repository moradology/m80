# CLI Scratch And Rootfs Effects

Behavior capture for bead `m80-lt15.17.5`.

## Rootfs Base

The selected runtime profile points at a shared read-only base rootfs. `m80 run`
does not mutate that base image. Per-run writes to `/` land in a disposable
overlay owned by the VM run directory.

This is the storage-pivot foundation: shared read-only base plus per-VM sparse
overlay rootfs. The CLI describes the user effect boundary; the block-device
details remain in storage/orchestration docs unless the user is inspecting a
run directory.

Reference:
`docs/design/storage-overlay.md`, `crates/m80-storage/README.md`, and
`crates/m80-firecracker/README.md`.

## Scratch Size

`--scratch-size <bytes>` sets the sparse overlay size used for this run. When
omitted, the CLI uses 512 MiB. A zero-byte scratch size is rejected as a wrapper
configuration error before backend/preflight work starts.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options`,
`crates/m80-cli/src/cmds/tests.rs::run_workspace_and_scratch_map_to_sandbox_config`,
`crates/m80-cli/src/cmds/tests.rs::run_defaults_to_no_workspace_and_default_overlay_size`,
and
`crates/m80-cli/tests/output_error_contract.rs::config_wrapper_failure_uses_stderr_exit_code_and_empty_stdout`.

## Workspace Scratch

When `--workspace <dir>` is supplied, m80 hydrates that host tree into a
separate workspace scratch image and mounts it at `/workspace` in the guest.
This workspace scratch is distinct from the rootfs overlay.

Current CLI behavior implements no host writeback by default. The child can
write inside the guest workspace during the run, but host mutation requires an
explicit writeback policy: `--writeback on-success` or `--writeback always`.

## ENOSPC

If the guest exhausts the rootfs overlay or workspace scratch capacity, the
write fails inside the guest. The CLI must preserve the child-vs-wrapper
boundary: a normal child ENOSPC is child process behavior, while a host-side
scratch creation/extraction failure is a wrapper failure.

## Preservation

`--keep-on-failure` is reserved and exits with feature-gap code 7 today. Until
that retention lane lands, run-dir preservation is a lower-level diagnostic
behavior, not a CLI guarantee for failed `m80 run` invocations.

Verification:
`crates/m80-cli/tests/feature_gap_smoke.rs::run_keep_on_failure_is_explicit_feature_gap`.

## Non-Commit Boundary

`--writeback` controls only workspace change extraction. It never commits rootfs
overlay mutations back into the selected image/profile. A future rootfs-export
feature would need a separate product surface and separate safety policy.
