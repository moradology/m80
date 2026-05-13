# Jailer Pid-File Backoff

After `MaterializedJail::launch` spawns the official Firecracker jailer, m80
waits for `<jail_root>/firecracker.pid`. That file is the handoff point for
the Firecracker process PID when the jailer forks, daemonizes, or enters a new
PID namespace.

The wait is bounded to one second. Missing pid-file checks use exponential
backoff starting at 1 ms, then 2 ms, 4 ms, 8 ms, 16 ms, and capped at 25 ms.
This keeps the timeout behavior bounded without imposing the old fixed 25 ms
minimum wait on the common path where the jailer writes the pid file quickly.

If the pid file does not appear before the deadline, m80 kills and reaps the
spawned child before returning `JailerError::FirecrackerPidTimeout`. If the
file appears but contains invalid data, launch fails closed with an I/O error
tagged to the pid-file path.

Tests:
- `crates/m80-jailer/src/materialized.rs::tests::firecracker_pid_poll_backoff_starts_at_one_ms_and_caps_at_twenty_five_ms`
- `crates/m80-jailer/src/materialized.rs::tests::firecracker_pid_wait_rechecks_after_one_ms_poll`
- `crates/m80-jailer/tests/jailer/pid_file_backoff.rs::launch_observes_pid_file_without_fixed_twenty_five_ms_floor`

Bench evidence:
- Baseline snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-13T15:43:07+00:00.json`
- After snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-13T16:04:00+00:00.json`
- Command: `N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh`
- `phase_9_jailer_launch` P50 changed from 25,437 us to 15,588 us (-9,849 us, -38.7%).
