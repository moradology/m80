# Two-Tenant Attack Runner Harness

The cross-tenant defense battery uses two simultaneously materialized
`m80-attack-runner` jails with distinct uid/gid pairs. This is the fixture
shape for later tests where one jailed payload treats the other tenant's
run-dir, sentinel, network state, or pid as the forbidden peer target.

The fixture keeps the official m80 launch boundary intact:

- both payloads are launched through `m80-jailer::MaterializedJail::launch`;
- the official Firecracker jailer receives the same env-cleared process shape;
- peer inputs are provided by a read-only bind at `/m80-attack-runner.conf`;
- each tenant gets its own run-dir and uid/gid pair.

The config file is intentionally plain `key=value` data because it is a test
fixture, not a new user-facing protocol. The current required peer keys are
`peer_sentinel`, `peer_run_dir`, `peer_network_state`, and `peer_pid`.

Evidence:

- `crates/m80-jailer/tests/defense_in_depth.rs::attack_runner_peer_config_transport_survives_env_clear`
- `crates/m80-jailer/tests/defense_in_depth.rs::two_tenant_attack_runner_fixture_materializes_distinct_live_jails`
