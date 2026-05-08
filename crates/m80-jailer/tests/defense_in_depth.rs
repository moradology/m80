#[path = "defense_in_depth/support.rs"]
mod support;

use std::path::PathBuf;

use m80_cgroup::Limits;
use support::*;

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_echo_zero_negative_control_reports_breach() {
    let result = run_attack_in_jailer("echo_zero").expect("run attack");

    assert_eq!(
        result.exit_code,
        Some(0),
        "echo_zero must prove the harness sees a successful attack as a breach"
    );
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_unknown_attack_reports_blocked() {
    let result = run_attack_in_jailer("missing_attack").expect("run attack");

    assert_ne!(
        result.exit_code,
        Some(0),
        "unknown attack must exercise the harness blocked/nonzero path"
    );
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_peer_config_transport_survives_env_clear() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_path = temp.path().join("attack-runner.conf");
    write_attack_config(
        &config_path,
        AttackConfig {
            peer_sentinel: "/peer/sentinel",
            peer_run_dir: "/peer/run",
            peer_network_state: "/peer/network-state.json",
            peer_pid: 42,
        },
    )
    .expect("write attack config");

    let result = run_attack_in_jailer_with_bindings(
        "require_peer_config",
        vec![config_binding(config_path)],
    )
    .expect("run attack with config");

    assert_eq!(
        result.exit_code,
        Some(0),
        "require_peer_config proves fixed-file config survived env_clear"
    );
}

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_can_be_enrolled_in_m80_cgroup_limits() {
    let result = run_attack_in_jailer_with_cgroup("sleep_briefly", Limits::m80_default())
        .expect("run cgroup-enrolled attack");

    assert_eq!(
        result.exit_code,
        Some(0),
        "sleep_briefly must preserve the live harness control"
    );
    assert!(
        result.cgroup_path.is_some(),
        "cgroup enrollment must record cgroup-path.txt"
    );
    assert!(
        result.cgroup_contained_pid,
        "cgroup.procs must contain the jailed attack-runner pid before wait"
    );
}

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_exhaust_file_descriptors() {
    assert_resource_attack_blocked("open_many_file_descriptors");
}

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_spawn_past_pids_limit() {
    assert_resource_attack_blocked("spawn_many_threads");
}

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_allocate_past_memory_limit() {
    assert_resource_attack_blocked("allocate_large_memory");
}

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_write_past_file_size_limit() {
    assert_file_size_attack_blocked();
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn two_tenant_attack_runner_fixture_materializes_distinct_live_jails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tenant_a = TenantSpec::new(temp.path(), "tenant-a", 3000, 3000);
    let tenant_b = TenantSpec::new(temp.path(), "tenant-b", 3001, 3001);

    write_attack_config(
        &tenant_a.config_path,
        AttackConfig {
            peer_sentinel: "/peer-b/sentinel",
            peer_run_dir: "/peer-b/run",
            peer_network_state: "/peer-b/network-state.json",
            peer_pid: 1,
        },
    )
    .expect("write tenant-a config");
    write_attack_config(
        &tenant_b.config_path,
        AttackConfig {
            peer_sentinel: "/peer-a/sentinel",
            peer_run_dir: "/peer-a/run",
            peer_network_state: "/peer-a/network-state.json",
            peer_pid: 1,
        },
    )
    .expect("write tenant-b config");

    let live_a = launch_attack_in_jailer(
        "sleep_briefly",
        &tenant_a.run_dir,
        tenant_a.uid,
        tenant_a.gid,
        vec![config_binding(tenant_a.config_path.clone())],
        None,
    )
    .expect("launch tenant-a attack");
    let live_b = launch_attack_in_jailer(
        "sleep_briefly",
        &tenant_b.run_dir,
        tenant_b.uid,
        tenant_b.gid,
        vec![config_binding(tenant_b.config_path.clone())],
        None,
    )
    .expect("launch tenant-b attack");

    assert_ne!(tenant_a.uid, tenant_b.uid, "tenants must use distinct uids");
    assert_ne!(tenant_a.gid, tenant_b.gid, "tenants must use distinct gids");
    assert_ne!(
        live_a.jailed.firecracker_pid, live_b.jailed.firecracker_pid,
        "two tenants must be distinct host processes"
    );
    assert!(
        proc_pid_exists(live_a.jailed.firecracker_pid)
            && proc_pid_exists(live_b.jailed.firecracker_pid),
        "both attack-runner processes must be live before cross-tenant tests run"
    );

    let result_a = live_a.wait().expect("wait tenant-a attack");
    let result_b = live_b.wait().expect("wait tenant-b attack");
    assert_eq!(result_a.exit_code, Some(0));
    assert_eq!(result_b.exit_code, Some(0));
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_chroot_escape_via_dotdot() {
    assert_attack_blocked("chroot_escape_via_dotdot");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_chroot_escape_via_openat_style_path() {
    assert_attack_blocked("chroot_escape_via_openat_style_path");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_chroot_escape_via_proc_self_root() {
    assert_attack_blocked("chroot_escape_via_proc_self_root");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_host_sentinel() {
    assert_attack_blocked("read_host_sentinel");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_write_host_sentinel() {
    assert_attack_blocked("write_host_sentinel");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_write_to_lower_layer() {
    assert_attack_blocked("write_to_lower_layer");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_observe_host_pid_status() {
    assert_attack_blocked("observe_host_pid_status");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_signal_host_pid_probe() {
    assert_attack_blocked("signal_host_pid_probe");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_host_pid_cmdline() {
    assert_attack_blocked("read_host_pid_cmdline");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_enumerate_host_processes() {
    assert_attack_blocked("enumerate_host_processes");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_host_proc_mountinfo() {
    assert_attack_blocked("read_host_proc_mountinfo");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_become_uid_zero() {
    assert_attack_blocked("become_uid_zero");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_become_gid_zero() {
    assert_attack_blocked("become_gid_zero");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_retain_effective_capabilities() {
    assert_attack_blocked("retain_effective_capabilities");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_unshare_mount_namespace() {
    assert_attack_blocked("unshare_mount_namespace");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mount_tmpfs() {
    assert_attack_blocked("mount_tmpfs");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_change_hostname() {
    assert_attack_blocked("change_hostname");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_open_raw_socket() {
    assert_attack_blocked("open_raw_socket");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_open_raw_packet_socket() {
    assert_attack_blocked("raw_packet_inject");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mutate_links_over_netlink() {
    assert_attack_blocked("send_arbitrary_netlink");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_bind_host_only_address() {
    assert_attack_blocked("bind_on_host_interface");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mutate_routes_over_netlink() {
    assert_attack_blocked("privileged_route_mutation");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_peer_sentinel() {
    assert_cross_tenant_attack_blocked("read_peer_sentinel", Vec::new());
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_write_peer_sentinel() {
    assert_cross_tenant_attack_blocked("write_peer_sentinel", Vec::new());
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_list_peer_run_dir() {
    assert_cross_tenant_attack_blocked("list_peer_run_dir", Vec::new());
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_peer_network_state() {
    assert_cross_tenant_attack_blocked("read_peer_network_state", Vec::new());
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_signal_peer_pid() {
    assert_cross_tenant_attack_blocked("signal_peer_pid", Vec::new());
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mount_peer_run_dir() {
    assert_cross_tenant_attack_blocked(
        "mount_peer_run_dir",
        vec![create_inside_jail(PathBuf::from("m80-peer-mount-target"))],
    );
}
