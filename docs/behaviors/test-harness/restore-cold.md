# Cold Restore Harness

Bead: `m80-jp6ik.43`.

The cold restore harness is `scripts/bench-restore-cold.sh`. It builds
`crates/m80-firecracker/benches/snapshot_restore_latency.rs`, runs the bench
binary under `sudo`, and writes
`crates/m80-firecracker/benches/snapshots/cold-restore-N${N}.json`.

The bench captures one golden snapshot, then measures:

- warm-cache `Sandbox::launch_from_snapshot` latency,
- cold-cache `Sandbox::launch_from_snapshot` latency after dropping page cache,
- per-restore phase timings from `diagnostics.jsonl`,
- direct warm and cold reads of `vm.snap` and `mem.snap`.

The shell plumbing regression is in `scripts/test-bench-harness.sh`; it checks
that the restore harness exposes a dry-run plan without requiring sudo, KVM, or
image artifacts.
