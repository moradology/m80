# m80 Attack Runner

`m80-attack-runner` is a deliberately malicious test binary for L11
defense-in-depth coverage. The jailer harness runs it where Firecracker would
normally execute and asserts each attack exits non-zero.

Exit contract:

- exit `0`: the named attack succeeded; the jail is breached.
- exit non-zero: the attack was blocked; stderr contains the denial reason.
- `echo_zero`: harness negative control that always exits `0`.

The CLI accepts either `<attack-name>` or `--api-sock <attack-name>`. The
second form is deliberately present for the official Firecracker jailer path,
which always forwards `--api-sock` to the jailed executable.

The runner exposes stable lowercase snake-case attack names grouped into six
Layer-2 categories: filesystem, process, network, privilege, resource, and
cross-tenant. Direct fixtures can provide host or peer paths through sentinel
environment variables such as `M80_ATTACK_HOST_SENTINEL`,
`M80_ATTACK_PEER_SENTINEL`, `M80_ATTACK_LOWER_SENTINEL`, and
`M80_ATTACK_HOST_PID`. The official jailer harness clears the child
environment, so root tests that need peer inputs bind a read-only config file
at `/m80-attack-runner.conf` instead.

The crate compiles for `x86_64-unknown-linux-musl` so the harness can copy one
static payload into a minimal jail without a dynamic linker dependency.
The executable payload is behind the `malicious-artifact` feature; default
workspace tests compile the library/catalog surface, and payload/CLI checks
enable that feature explicitly.

Verification:

- `cargo test -p m80-attack-runner --features malicious-artifact`
- `cargo clippy -p m80-attack-runner --features malicious-artifact --all-targets -- -D warnings`
- `cargo build -p m80-attack-runner --features malicious-artifact --target x86_64-unknown-linux-musl`
