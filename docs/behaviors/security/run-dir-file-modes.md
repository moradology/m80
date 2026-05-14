# Run Directory File Modes

Behavior capture for bead `m80-8emae.12`.

## Contract

Per-VM run directories are owner-only:

```text
<run_root>/<vm_id>/ 0700
```

The host-visible files most likely to carry diagnostic paths, kernel/rootfs
identity, guest serial output, or lifecycle context are created owner-only:

```text
<run_dir>/diagnostics.jsonl  0600
<run_dir>/boot-identity.json 0600
<run_dir>/console.log        0600
```

These modes are enforced when m80 opens the directory or file, including stale
files from earlier runs that already exist with broader permissions. Existing
callers still read the same paths; this is a permission hardening change, not a
layout change.

## Non-Contract

This does not make run-root a multi-tenant service boundary. Same-UID processes
can still read owner files, and root can read everything. Group-readable
operator logs are not part of the v0.x default; a future service wrapper can
copy or export selected records through an explicit policy.

## Verification

- `crates/m80-observability/src/diagnostics.rs::tests::diagnostics_log_is_owner_only`
- `crates/m80-firecracker/src/boot_identity.rs::tests::record_writes_owner_only_boot_identity`
- `crates/m80-firecracker/src/launch/tests.rs::phase_1_run_root_prep_creates_owner_only_run_dir`
- `crates/m80-jailer/src/materialized_tests.rs::launch_redirects_stdio_and_passes_hardening_args`
