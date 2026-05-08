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
