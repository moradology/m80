# Process and PID Isolation Battery

The L11 process/PID battery runs five `m80-attack-runner` primitives through
the official Firecracker jailer harness. Each test asserts the attack exits
non-zero, meaning the jail blocked host process observation or signaling.

Covered attacks:

- `observe_host_pid_status`
- `signal_host_pid_probe`
- `read_host_pid_cmdline`
- `enumerate_host_processes`
- `read_host_proc_mountinfo`

The tests are ignored because they require root, the official jailer,
`m80-jailer-harden`, and a musl-built attack runner.

Verification:

- `cargo test -p m80-jailer --test defense_in_depth --no-run`
- root/manual: `cargo test -p m80-jailer --test defense_in_depth -- --ignored jailed_attacker_cannot_`
