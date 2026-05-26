# No-KVM launch edge coverage

Bead: `m80-vpw49.1`

This behavior capture pins launch and snapshot-lifecycle edges that are
security- or operator-facing but do not require a real Firecracker boot to
test.

## Restore probe request identity

The snapshot-restore ready probe sends a guest exec request with an opaque
restore-probe `request_id`. Every response frame consumed by the probe must
carry the same `request_id`; missing or mismatched ids fail closed as
`WireProtocolError::RequestIdMismatch`.

The retry loop retries only vsock transport failures. Protocol errors return
immediately. If vsock transport failures continue until the restore-probe
deadline, the phase returns `FcError::GuestdReadyTimeout` with the probed UDS
path and timeout.

Tests:
- `restore_probe_accepts_matching_request_id`
- `restore_probe_rejects_missing_request_id`
- `restore_probe_rejects_mismatched_request_id`
- `restore_probe_retries_vsock_failures_until_success`
- `restore_probe_timeout_reports_guestd_ready_timeout`
- `restore_probe_does_not_retry_protocol_errors`

## Join-netns guest cmdline

For `RealizedNetwork::JoinNetns`, phase 7 emits exactly the PID-1 network
tokens needed for the guest static interface contract, in stable order:

1. `m80.net=join_netns`
2. `m80.net.iface=eth0`
3. `m80.net.mac=<mac>`
4. `m80.net.ipv4=<ipv4/prefix>`
5. `m80.net.gateway=<gateway>`
6. `m80.net.dns=<comma-separated resolvers>`

Test: `join_netns_guest_config_emits_stable_pid_one_tokens`.

## Cgroup v2 probe errors

`CgroupMode::Disabled` does not probe the host. When `UnifiedV2` is requested,
`UnsupportedHostMode` is reclassified to a config error on `cgroup_mode` so the
operator sees that the requested mode is impossible on the host. Other cgroup
errors stay typed as `FcError::Cgroup`.

Tests:
- `cgroup_probe_disabled_skips_host_probe`
- `cgroup_probe_unsupported_host_mode_is_config_error`
- `cgroup_probe_other_errors_keep_cgroup_variant`

## Snapshot path staging

Snapshot capture and restore use one host snapshot directory that is mounted
into the jail at `/snapshot`. The `vm_state` and `mem` files must share the same
host parent directory. Prepared jail paths preserve the file names while moving
them under `/snapshot`.

Tests:
- `prepare_snapshot_paths_rejects_split_parent_pair`
- `prepare_snapshot_paths_maps_pair_into_jail_snapshot_dir`
