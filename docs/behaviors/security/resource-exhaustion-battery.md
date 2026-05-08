# Resource Exhaustion Battery

The Layer 2 resource battery runs jailed `m80-attack-runner` payloads after
the harness enrolls the live process in an `m80-cgroup::Subtree`. Each attack
must exit non-zero because a configured resource boundary stopped it before it
could consume unbounded host resources.

Pinned attacks:

- `open_many_file_descriptors` opens `/dev/null` repeatedly until
  `RLIMIT_NOFILE` returns `EMFILE`.
- `spawn_many_threads` creates tasks until the cgroup `pids.max` boundary
  rejects new tasks.
- `allocate_large_memory` touches anonymous memory until the cgroup
  `memory.max` boundary kills or rejects the process.
- `create_large_tmp_file` writes real bytes until `RLIMIT_FSIZE` rejects the
  write or terminates the process.

The ignored root tests also assert `cgroup.procs` contained the attack-runner
pid before wait, so a non-zero exit is tied to the enrolled resource harness
rather than a standalone process failure.

Evidence:

- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_exhaust_file_descriptors`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_spawn_past_pids_limit`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_allocate_past_memory_limit`
- `crates/m80-jailer/tests/defense_in_depth.rs::jailed_attacker_cannot_write_past_file_size_limit`
