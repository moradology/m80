# Concurrency — Snapshot Capture And Restore

Snapshot files are caller-selected paths, so concurrent use of the same
snapshot parent must fail loudly or leave a complete snapshot pair. m80 does
not silently treat partial `vm_state` or `mem` output as a usable warm-start
source.

The real-KVM regression creates an initial golden snapshot, starts one VM that
restores from that snapshot parent, and concurrently asks a second VM to capture
back into the same `vm.snap` and `mem.snap` paths. Both threads must complete
without deadlock. After the race, both snapshot files must still be non-empty,
and a final restore from the same paths must boot and execute a command that
reads the expected in-guest marker.

Test:
`crates/m80-firecracker/tests/snapshot_concurrency_real_kvm.rs::snapshot_concurrent_capture_and_restore`
is ignored by default because it requires KVM and Firecracker snapshot support.
