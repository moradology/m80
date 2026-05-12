# `m80-attack-runner`

Small malicious binary used by the defense-in-depth test harness. It runs one
named attack and exits:

- `0` when the attack succeeded, meaning the jail was breached.
- `1` when the attack was blocked, printing the block reason to stderr.
- `2` when the invocation is invalid. Missing attack names print usage and the
  known attack list to stderr.

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
attach the live payload process before it terminates. `require_peer_config`
exits `0` only when the fixed in-jail config file contains the peer sentinel,
run-dir, network-state, and pid keys needed by two-tenant tests.

When the runner is launched through `m80-jailer`, the official launch path
clears the child environment. The root harness therefore bind-mounts a
read-only config file at `/m80-attack-runner.conf` and uses these keys:

- `host_sentinel`
- `host_pid`
- `lower_sentinel`
- `peer_sentinel`
- `peer_run_dir`
- `peer_network_state`
- `peer_pid`

## Public surface

The package keeps a library target because the binary, catalog tests, and
jailer harness tests need to inspect or invoke the stable attack catalog without
spawning a subprocess for every assertion.

- `Attack` — one registered attack primitive with stable `name` and
  `category` fields.
- `AttackCategory` — category enum used by defense-in-depth batteries:
  `Filesystem`, `Process`, `Network`, `Privilege`, `Resource`,
  `CrossTenant`.
- `AttackBlocked` — structured block reason returned when a kernel or jail
  policy prevented the attempted breach.
- `AttackBlocked::new(reason)` — constructs a block reason.
- `AttackBlocked::reason()` — returns the human-readable block reason.
- `AttackResult` — alias for `Result<(), AttackBlocked>`.
- `attack_names()` — returns all stable attack names, excluding harness
  controls such as `echo_zero`.
- `attacks_by_category()` — returns stable attack names grouped by category.
- `run_attack(name)` — runs one stable attack or harness control by name.

## Adding an Attack

Add the function in `src/attacks/<category>.rs`, then register it in
`src/catalog.rs` with a stable lowercase snake-case name. Names are part of the
test contract; do not rename an attack without updating the corresponding bead
and test.

Direct non-jailer fixtures may still use environment variables for
harness-provided host or tenant sentinel paths and PIDs:

- `M80_ATTACK_HOST_SENTINEL`
- `M80_ATTACK_PEER_SENTINEL`
- `M80_ATTACK_LOWER_SENTINEL`
- `M80_ATTACK_HOST_PID`
- `M80_ATTACK_PEER_RUN_DIR`
- `M80_ATTACK_PEER_NETWORK_STATE`
- `M80_ATTACK_PEER_PID`

Each attack returns `Ok(())` only when it observed the forbidden capability.
Expected kernel or filesystem denials return `AttackBlocked`.

Network attacks are split by the current m80 boundary. Ordinary TCP connect or
listen probes model guest-egress policy and are not used by the compromised-VMM
Layer 2 battery. They stay in this crate as the catalog home for egress-policy
tests: `connect_imds_http`, `connect_public_dns_tcp`,
`connect_private_rfc1918`, `bind_privileged_port`, and
`listen_all_interfaces`. The privileged network probes (`open_raw_socket`,
`raw_packet_inject`, `send_arbitrary_netlink`, `bind_on_host_interface`, and
`privileged_route_mutation`) are the stable names for jailer/capability-drop
coverage.

Cross-tenant probes use the peer config keys above. The stable cross-tenant
names are `read_peer_sentinel`, `write_peer_sentinel`, `list_peer_run_dir`,
`read_peer_network_state`, `signal_peer_pid`, and `mount_peer_run_dir`.

Resource probes are intentionally bounded by the jailer harness. The stable
names are `open_many_file_descriptors`, `spawn_many_threads`,
`allocate_large_memory`, and `create_large_tmp_file`.
