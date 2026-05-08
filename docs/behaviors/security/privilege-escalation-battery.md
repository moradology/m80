# Privilege Escalation Battery

The L11 privilege battery runs six `m80-attack-runner` primitives through the
official Firecracker jailer harness. Each test asserts the attack exits
non-zero, meaning m80's hardening plus the official jailer blocked privilege
gain or privileged namespace/sysctl operations.

Covered attacks:

- `become_uid_zero`
- `become_gid_zero`
- `retain_effective_capabilities`
- `unshare_mount_namespace`
- `mount_tmpfs`
- `change_hostname`

The tests are ignored because they require root, the official jailer,
`m80-jailer-harden`, and a musl-built attack runner.

Verification:

- `cargo test -p m80-jailer --test defense_in_depth --no-run`
- root/manual: `cargo test -p m80-jailer --test defense_in_depth -- --ignored jailed_attacker_cannot_`
