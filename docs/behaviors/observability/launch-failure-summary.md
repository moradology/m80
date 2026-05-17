# Launch Failure Summary

Cold launch and snapshot restore failures preserve triage evidence after the
run directory exists.

On an error return from `Sandbox::launch` or `Sandbox::launch_from_snapshot`,
`m80-firecracker` writes `<run_dir>/failure_summary.json` before cleanup guards
run. The file is written through a temporary sibling and then renamed into
place. Its fields are:

- `vm_id`
- `failed_phase`
- `error_variant`
- `error_display`
- `timestamp_unix_ms`
- `request_id`

The launch cleanup guard preserves failed run directories under
`<run_root>/.preserved/<unix_ms>-<vm_id>/` by default. This keeps
`failure_summary.json`, `diagnostics.jsonl`, and `console.log` available after
the admission permit is released. Callers that want the older cleanup behavior
must opt in with `Sandbox::delete_run_dir_on_launch_error()` before launching.

Tests:

- `crates/m80-firecracker/src/diagnostics.rs::tests::failure_summary_is_written_atomically_with_variant`
- `crates/m80-firecracker/src/launch/failure_cleanup.rs::tests::failed_launch_guard_preserves_run_dir_by_default`
- `crates/m80-firecracker/src/launch/failure_cleanup.rs::tests::failed_launch_guard_deletes_when_requested`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::api_socket_timeout_cleans_partial_state`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::cgroup_create_failure_mid_launch_cleans_partial_state_and_releases_permit`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::guestd_not_ready_timeout_cleans_partial_state`
- `crates/m80-firecracker/tests/lifecycle_failure_real_kvm.rs::restore_guestd_not_ready_timeout_cleans_partial_state`
