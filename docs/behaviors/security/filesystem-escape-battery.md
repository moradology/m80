# Filesystem Escape Battery

The L11 filesystem battery runs six `m80-attack-runner` primitives through the
official Firecracker jailer harness. Each test asserts the attack exits
non-zero, meaning the jail blocked the forbidden filesystem observation or
mutation.

Covered attacks:

- `chroot_escape_via_dotdot`
- `chroot_escape_via_openat_style_path`
- `chroot_escape_via_proc_self_root`
- `read_host_sentinel`
- `write_host_sentinel`
- `write_to_lower_layer`

These tests are ignored because they require root, the official jailer,
`m80-jailer-harden`, and a musl-built attack runner. They use the same
`run_attack_in_jailer` helper documented in
`docs/behaviors/security/attack-runner-harness.md`.

Verification:

- `cargo test -p m80-jailer --test defense_in_depth --no-run`
- root/manual: `cargo test -p m80-jailer --test defense_in_depth -- --ignored jailed_attacker_cannot_`
