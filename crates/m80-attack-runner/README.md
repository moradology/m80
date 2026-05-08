# `m80-attack-runner`

Small malicious binary used by the defense-in-depth test harness. It runs one
named attack and exits:

- `0` when the attack succeeded, meaning the jail was breached.
- non-zero when the attack was blocked, printing the block reason to stderr.

The runner does not claim the host is safe by itself. It is an executable
payload for later jailer tests, which decide where the binary runs and which
sentinel paths are exposed.

For the official Firecracker jailer harness, the runner also accepts
`--api-sock <attack-name>`. m80's jailer launch path always forwards
`--api-sock` to the jailed executable, so the harness reuses that field as the
attack selector instead of adding a second launch API.

Harness controls are intentionally outside `attack_names()`: `echo_zero` exits
`0` immediately to prove the harness detects a successful attack, and
`sleep_briefly` exits `0` after a short delay so cgroup-enrollment tests can
attach the live payload process before it terminates.

## Adding an Attack

Add the function in `src/attacks/<category>.rs`, then register it in
`src/catalog.rs` with a stable lowercase snake-case name. Names are part of the
test contract; do not rename an attack without updating the corresponding bead
and test.

Use environment variables for harness-provided host or tenant sentinel paths:

- `M80_ATTACK_HOST_SENTINEL`
- `M80_ATTACK_PEER_SENTINEL`
- `M80_ATTACK_LOWER_SENTINEL`
- `M80_ATTACK_HOST_PID`

Each attack returns `Ok(())` only when it observed the forbidden capability.
Expected kernel or filesystem denials return `AttackBlocked`.

Network attacks are split by the current m80 boundary. Ordinary TCP connect or
listen probes model guest-egress policy and are not used by the compromised-VMM
Layer 2 battery. The privileged network probes (`open_raw_socket`,
`raw_packet_inject`, `send_arbitrary_netlink`, `bind_on_host_interface`, and
`privileged_route_mutation`) are the stable names for jailer/capability-drop
coverage.
