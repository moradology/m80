#[path = "defense_in_depth/support.rs"]
mod support;

use std::path::PathBuf;

use support::*;

const STANDARD_LAYER2_ATTACKS: &[&str] = &[
    "chroot_escape_via_dotdot",
    "chroot_escape_via_openat_style_path",
    "chroot_escape_via_proc_self_root",
    "read_host_sentinel",
    "write_host_sentinel",
    "write_to_lower_layer",
    "observe_host_pid_status",
    "signal_host_pid_probe",
    "read_host_pid_cmdline",
    "enumerate_host_processes",
    "read_host_proc_mountinfo",
    "open_raw_socket",
    "raw_packet_inject",
    "send_arbitrary_netlink",
    "bind_on_host_interface",
    "privileged_route_mutation",
    "become_uid_zero",
    "become_gid_zero",
    "retain_effective_capabilities",
    "unshare_mount_namespace",
    "mount_tmpfs",
    "change_hostname",
];

const RESOURCE_CGROUP_ATTACKS: &[&str] = &[
    "open_many_file_descriptors",
    "spawn_many_threads",
    "allocate_large_memory",
];

const CROSS_TENANT_ATTACKS: &[&str] = &[
    "read_peer_sentinel",
    "write_peer_sentinel",
    "list_peer_run_dir",
    "read_peer_network_state",
    "signal_peer_pid",
];

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn compromised_fc_blocked_by_jailer_alone() {
    for name in STANDARD_LAYER2_ATTACKS {
        assert_attack_blocked(name);
    }

    for name in RESOURCE_CGROUP_ATTACKS {
        assert_resource_attack_blocked(name);
    }
    assert_file_size_attack_blocked();

    for name in CROSS_TENANT_ATTACKS {
        assert_cross_tenant_attack_blocked(name, Vec::new());
    }
    assert_cross_tenant_attack_blocked(
        "mount_peer_run_dir",
        vec![create_inside_jail(PathBuf::from("m80-peer-mount-target"))],
    );
}
