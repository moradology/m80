# Cross-Tenant Attack Battery

The Layer 2 cross-tenant battery runs one jailed `m80-attack-runner` payload as
the attacker while a second payload stays live as the peer tenant. The two
payloads use distinct uid/gid pairs and distinct jail roots.

The attacker receives a read-only bind of the test root at `/peers`, but the
peer-private directory under that root is owned by the peer uid/gid and has
mode `0700`. This deliberately exposes a host-side peer path to the attacker
fixture so the test proves the peer permission boundary, not only chroot
absence.

The named attack tests must exit non-zero:

- `read_peer_sentinel`
- `write_peer_sentinel`
- `list_peer_run_dir`
- `read_peer_network_state`
- `signal_peer_pid`
- `mount_peer_run_dir`

The signal test also asserts the peer payload is still live after the attacker
returns. A zero exit from any named attack means the compromised-Firecracker
defense-in-depth boundary has failed.

Evidence:

- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_read_peer_sentinel`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_write_peer_sentinel`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_list_peer_run_dir`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_read_peer_network_state`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_signal_peer_pid`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_mount_peer_run_dir`
