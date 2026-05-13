# Behavior: Snapshot File Priming

**Bead:** `m80-jp6ik.34`
**Date:** 2026-05-13
**Test:** `crates/m80-firecracker/src/launch/tests.rs`

## Contract

`Sandbox::launch_from_snapshot` calls `posix_fadvise(POSIX_FADV_WILLNEED)` on
the host-visible `vm.snap` and `mem.snap` files before binding their parent
directory into the Firecracker jail and before issuing `PUT /snapshot/load`.

The priming point uses the caller's original host paths, not the jail-visible
`/snapshot/...` paths passed to Firecracker. Missing or unreadable snapshot
files fail with a path-carrying `FcError::PathIo`; m80 does not silently fall
back to an unprimed restore.

## Measurement Scope

This is page-cache advice, not a synchronous full-file read. Linux may queue
readahead and return before the entire `mem.snap` is resident. Restore latency
evidence must come from the real restore harness, not from direct full-file
read timings alone.
