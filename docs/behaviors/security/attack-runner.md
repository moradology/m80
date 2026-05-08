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
cross-tenant. Host or peer fixtures are provided through sentinel environment
variables such as `M80_ATTACK_HOST_SENTINEL`, `M80_ATTACK_PEER_SENTINEL`,
`M80_ATTACK_LOWER_SENTINEL`, and `M80_ATTACK_HOST_PID`.

The crate compiles for `x86_64-unknown-linux-musl` so the harness can copy one
static payload into a minimal jail without a dynamic linker dependency.

Verification:

- `cargo test -p m80-attack-runner`
- `cargo clippy -p m80-attack-runner --all-targets -- -D warnings`
- `cargo build -p m80-attack-runner --target x86_64-unknown-linux-musl`
