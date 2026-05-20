# Cgroup-Enrolled Attack Runner

## Behavior

The defense-in-depth jailer harness can launch `m80-attack-runner` through the
official Firecracker jailer path and then enroll that live process in the same
`m80-cgroup` subtree shape used by `m80-firecracker` when
`CgroupMode::UnifiedV2` is enabled.

The harness uses the `sleep_briefly` control so the payload remains alive long
enough to call `Subtree::create`. It passes `Limits::preset()`, reads the
recorded `cgroup-path.txt`, and verifies `cgroup.procs` contains the jailed
attack-runner PID before waiting for process exit. The existing `echo_zero`
negative control remains unchanged and continues to prove the harness detects a
successful attack as a breach.

This bead proves resource-exhaustion attacks can run under production-equivalent
cgroup limits. The resource attack battery itself remains a separate leaf.

## Evidence

- `crates/m80-attack-runner/src/catalog.rs` exposes the `sleep_briefly` harness
  control outside the stable attack catalog.
- `crates/m80-jailer/tests/defense_in_depth.rs::attack_runner_can_be_enrolled_in_m80_cgroup_limits`
  launches the payload, enrolls it with `m80-cgroup`, and checks
  `cgroup.procs`.
- `crates/m80-jailer/Cargo.toml` carries `m80-cgroup` as a dev-dependency so
  production `m80-jailer` stays decoupled from cgroup ownership.

## Verification

Default verification compiles the ignored root-only harness:

- `cargo test -p m80-attack-runner --features malicious-artifact`
- `cargo test -p m80-jailer --test defense_in_depth --no-run`
- `cargo clippy -p m80-attack-runner --features malicious-artifact --all-targets -- -D warnings`
- `cargo clippy -p m80-jailer --test defense_in_depth -- -D warnings`

Full execution requires root, a writable cgroup v2 hierarchy, the official
Firecracker jailer, `m80-jailer-harden`, and the musl `m80-attack-runner`
artifact.
