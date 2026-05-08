//! Stable attack catalog and dispatch.

use crate::attacks::{cross_tenant, filesystem, network, privilege, process, resource};
use crate::{AttackBlocked, AttackResult};

/// Defense-in-depth attack category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttackCategory {
    /// Filesystem escape attacks.
    Filesystem,
    /// Process and PID-isolation attacks.
    Process,
    /// Network isolation attacks.
    Network,
    /// Privilege escalation attacks.
    Privilege,
    /// Resource exhaustion attacks.
    Resource,
    /// Cross-tenant isolation attacks.
    CrossTenant,
}

/// One registered attack primitive.
#[derive(Clone, Copy)]
pub struct Attack {
    /// Stable attack name used on the CLI.
    pub name: &'static str,
    /// Category used by the L11 battery tests.
    pub category: AttackCategory,
    run: fn() -> AttackResult,
}

impl Attack {
    const fn new(name: &'static str, category: AttackCategory, run: fn() -> AttackResult) -> Self {
        Self {
            name,
            category,
            run,
        }
    }
}

const ATTACKS: &[Attack] = &[
    Attack::new(
        "chroot_escape_via_dotdot",
        AttackCategory::Filesystem,
        filesystem::chroot_escape_via_dotdot,
    ),
    Attack::new(
        "chroot_escape_via_openat_style_path",
        AttackCategory::Filesystem,
        filesystem::chroot_escape_via_openat_style_path,
    ),
    Attack::new(
        "chroot_escape_via_proc_self_root",
        AttackCategory::Filesystem,
        filesystem::chroot_escape_via_proc_self_root,
    ),
    Attack::new(
        "read_host_sentinel",
        AttackCategory::Filesystem,
        filesystem::read_host_sentinel,
    ),
    Attack::new(
        "write_host_sentinel",
        AttackCategory::Filesystem,
        filesystem::write_host_sentinel,
    ),
    Attack::new(
        "write_to_lower_layer",
        AttackCategory::Filesystem,
        filesystem::write_to_lower_layer,
    ),
    Attack::new(
        "observe_host_pid_status",
        AttackCategory::Process,
        process::observe_host_pid_status,
    ),
    Attack::new(
        "signal_host_pid_probe",
        AttackCategory::Process,
        process::signal_host_pid_probe,
    ),
    Attack::new(
        "read_host_pid_cmdline",
        AttackCategory::Process,
        process::read_host_pid_cmdline,
    ),
    Attack::new(
        "enumerate_host_processes",
        AttackCategory::Process,
        process::enumerate_host_processes,
    ),
    Attack::new(
        "read_host_proc_mountinfo",
        AttackCategory::Process,
        process::read_host_proc_mountinfo,
    ),
    Attack::new(
        "connect_imds_http",
        AttackCategory::Network,
        network::connect_imds_http,
    ),
    Attack::new(
        "connect_public_dns_tcp",
        AttackCategory::Network,
        network::connect_public_dns_tcp,
    ),
    Attack::new(
        "connect_private_rfc1918",
        AttackCategory::Network,
        network::connect_private_rfc1918,
    ),
    Attack::new(
        "bind_privileged_port",
        AttackCategory::Network,
        network::bind_privileged_port,
    ),
    Attack::new(
        "listen_all_interfaces",
        AttackCategory::Network,
        network::listen_all_interfaces,
    ),
    Attack::new(
        "open_raw_socket",
        AttackCategory::Network,
        network::open_raw_socket,
    ),
    Attack::new(
        "raw_packet_inject",
        AttackCategory::Network,
        network::raw_packet_inject,
    ),
    Attack::new(
        "send_arbitrary_netlink",
        AttackCategory::Network,
        network::send_arbitrary_netlink,
    ),
    Attack::new(
        "bind_on_host_interface",
        AttackCategory::Network,
        network::bind_on_host_interface,
    ),
    Attack::new(
        "privileged_route_mutation",
        AttackCategory::Network,
        network::privileged_route_mutation,
    ),
    Attack::new(
        "become_uid_zero",
        AttackCategory::Privilege,
        privilege::become_uid_zero,
    ),
    Attack::new(
        "become_gid_zero",
        AttackCategory::Privilege,
        privilege::become_gid_zero,
    ),
    Attack::new(
        "retain_effective_capabilities",
        AttackCategory::Privilege,
        privilege::retain_effective_capabilities,
    ),
    Attack::new(
        "unshare_mount_namespace",
        AttackCategory::Privilege,
        privilege::unshare_mount_namespace,
    ),
    Attack::new(
        "mount_tmpfs",
        AttackCategory::Privilege,
        privilege::mount_tmpfs,
    ),
    Attack::new(
        "change_hostname",
        AttackCategory::Privilege,
        privilege::change_hostname,
    ),
    Attack::new(
        "open_many_file_descriptors",
        AttackCategory::Resource,
        resource::open_many_file_descriptors,
    ),
    Attack::new(
        "spawn_many_threads",
        AttackCategory::Resource,
        resource::spawn_many_threads,
    ),
    Attack::new(
        "allocate_large_memory",
        AttackCategory::Resource,
        resource::allocate_large_memory,
    ),
    Attack::new(
        "create_large_tmp_file",
        AttackCategory::Resource,
        resource::create_large_tmp_file,
    ),
    Attack::new(
        "read_peer_sentinel",
        AttackCategory::CrossTenant,
        cross_tenant::read_peer_sentinel,
    ),
    Attack::new(
        "write_peer_sentinel",
        AttackCategory::CrossTenant,
        cross_tenant::write_peer_sentinel,
    ),
    Attack::new(
        "list_peer_run_dir",
        AttackCategory::CrossTenant,
        cross_tenant::list_peer_run_dir,
    ),
    Attack::new(
        "read_peer_network_state",
        AttackCategory::CrossTenant,
        cross_tenant::read_peer_network_state,
    ),
    Attack::new(
        "signal_peer_pid",
        AttackCategory::CrossTenant,
        cross_tenant::signal_peer_pid,
    ),
    Attack::new(
        "mount_peer_run_dir",
        AttackCategory::CrossTenant,
        cross_tenant::mount_peer_run_dir,
    ),
];

/// Return all stable attack names, excluding harness controls.
pub fn attack_names() -> Vec<&'static str> {
    ATTACKS.iter().map(|attack| attack.name).collect()
}

/// Return attacks grouped by category.
pub fn attacks_by_category() -> Vec<(AttackCategory, Vec<&'static str>)> {
    let mut grouped = Vec::new();
    for category in [
        AttackCategory::Filesystem,
        AttackCategory::Process,
        AttackCategory::Network,
        AttackCategory::Privilege,
        AttackCategory::Resource,
        AttackCategory::CrossTenant,
    ] {
        grouped.push((
            category,
            ATTACKS
                .iter()
                .filter(|attack| attack.category == category)
                .map(|attack| attack.name)
                .collect(),
        ));
    }
    grouped
}

/// Run one attack by stable name.
pub fn run_attack(name: &str) -> AttackResult {
    if name == "echo_zero" {
        return Ok(());
    }
    if name == "sleep_briefly" {
        std::thread::sleep(std::time::Duration::from_secs(5));
        return Ok(());
    }
    if name == "require_peer_config" {
        return crate::require_peer_config();
    }
    let attack = ATTACKS
        .iter()
        .find(|attack| attack.name == name)
        .ok_or_else(|| AttackBlocked::new(format!("unknown attack {name}")))?;
    (attack.run)()
}
