# Attack Runner Jailer Harness

The defense-in-depth harness launches `m80-attack-runner` through the same
`m80-jailer` materialization and official Firecracker jailer path used for
Firecracker. The runner accepts the forwarded `--api-sock <attack-name>`
argument as its attack selector, which lets the harness reuse
`MaterializedJail::launch` without widening m80-jailer's public API into a
general chroot process runner.

Harness result contract:

- attack exit `0`: the attack succeeded; the harness must treat this as a
  breach.
- attack exit non-zero: the attack was blocked; the harness records the named
  attack as held by the jail.
- `echo_zero`: negative control that always exits `0`, proving the harness
  detects an unblocked attack path.

The ignored root tests in `crates/m80-jailer/tests/defense_in_depth.rs` require
the official jailer, `m80-jailer-harden`, and a musl-built attack runner. By
default they look for:

- `/usr/bin/jailer`
- `/usr/bin/m80-jailer-harden`
- `target/x86_64-unknown-linux-musl/debug/m80-attack-runner`

Each path can be overridden with `M80_JAILER_BIN`,
`M80_JAILER_HARDEN_BIN`, or `M80_ATTACK_RUNNER_BIN`.

Verification:

- `cargo test -p m80-jailer --test defense_in_depth --no-run`
- root/manual: `cargo test -p m80-jailer --test defense_in_depth -- --ignored`
