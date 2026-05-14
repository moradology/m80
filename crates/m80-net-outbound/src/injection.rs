#![allow(dead_code)]

use std::io::Write;
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;

use crate::{
    discover_dns_resolvers_with_ops, iptables::invalid_state, write_vm_network_state_record,
    CommandDnsDiscoveryOps, DnsCommandOutput, DnsDiscoveryOps, NetError, SetupPhase,
    VmNetworkStateRecord,
};

/// systemd-networkd directory inside the runtime rootfs image.
pub(crate) const SYSTEMD_NETWORK_DIR: &str = "/etc/systemd/network";

/// systemd-resolved drop-in directory inside the runtime rootfs image.
pub(crate) const SYSTEMD_RESOLVED_CONF_DIR: &str = "/etc/systemd/resolved.conf.d";

/// m80 networkd unit path inside the runtime rootfs image.
pub(crate) const NETWORKD_FILE: &str = "/etc/systemd/network/10-m80-outbound.network";

/// m80 resolved drop-in path inside the runtime rootfs image.
pub(crate) const RESOLVED_FILE: &str = "/etc/systemd/resolved.conf.d/10-m80-dns.conf";

/// Rendered guest networking configuration files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuestNetworkConfig {
    /// Contents of `10-m80-outbound.network`.
    pub networkd: String,
    /// Contents of `10-m80-dns.conf`.
    pub resolved: String,
}

/// Kernel command-line tokens consumed by `m80-guestd` PID-1 networking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidOneNetworkCmdline {
    /// Tokens to append to the guest kernel command line.
    pub args: Vec<String>,
}

/// Host seam for writing guest network configuration into an ext4 image.
pub(crate) trait GuestNetworkConfigOps: DnsDiscoveryOps {
    /// Run a host command whose non-zero exit aborts injection.
    fn run_command(&mut self, program: &str, args: &[String]) -> Result<(), NetError>;
}

/// Real-host backend for guest network config injection via `debugfs`.
pub(crate) struct CommandGuestNetworkConfigOps;

impl DnsDiscoveryOps for CommandGuestNetworkConfigOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<DnsCommandOutput, NetError> {
        let output = Command::new(program).args(args).output()?;
        Ok(DnsCommandOutput {
            status_success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn read_to_string(&mut self, path: &Path) -> Result<String, NetError> {
        Ok(std::fs::read_to_string(path)?)
    }
}

impl GuestNetworkConfigOps for CommandGuestNetworkConfigOps {
    fn run_command(&mut self, program: &str, args: &[String]) -> Result<(), NetError> {
        let output = self.command_output(program, args)?;
        if output.status_success {
            return Ok(());
        }
        Err(NetError::NetworkCommandFailed {
            program: program.to_owned(),
            stderr: output.stderr,
        })
    }
}

/// Discover DNS and inject guest network config through a supplied host seam.
pub(crate) fn inject_guest_network_config(
    state: &mut VmNetworkStateRecord,
    runtime_rootfs: &Path,
) -> Result<(), NetError> {
    let mut ops = CommandGuestNetworkConfigOps;
    inject_guest_network_config_with_ops(&mut ops, state, runtime_rootfs)
}

/// Discover DNS and inject guest network config through a supplied host seam.
pub(crate) fn inject_guest_network_config_with_ops(
    ops: &mut impl GuestNetworkConfigOps,
    state: &mut VmNetworkStateRecord,
    runtime_rootfs: &Path,
) -> Result<(), NetError> {
    validate_ready_state(state)?;
    let resolvers = discover_dns_resolvers_with_ops(ops)?;
    let config = build_guest_network_config(state, &resolvers)?;
    write_guest_network_config(ops, runtime_rootfs, &config)?;

    state.dns_resolvers = resolvers;
    state.runtime_rootfs_configured = true;
    write_vm_network_state_record(&state.run_dir, state)
}

/// Discover DNS and prepare PID-1 network command-line tokens.
pub fn prepare_pid_one_network_cmdline(
    state: &mut VmNetworkStateRecord,
) -> Result<PidOneNetworkCmdline, NetError> {
    let mut ops = CommandDnsDiscoveryOps;
    prepare_pid_one_network_cmdline_with_ops(&mut ops, state)
}

/// Discover DNS and prepare PID-1 network command-line tokens through a seam.
pub(crate) fn prepare_pid_one_network_cmdline_with_ops(
    ops: &mut impl DnsDiscoveryOps,
    state: &mut VmNetworkStateRecord,
) -> Result<PidOneNetworkCmdline, NetError> {
    validate_ready_state(state)?;
    let resolvers = discover_dns_resolvers_with_ops(ops)?;
    state.dns_resolvers = resolvers;
    state.runtime_rootfs_configured = true;
    let cmdline = build_pid_one_network_cmdline(state)?;
    write_vm_network_state_record(&state.run_dir, state)?;
    Ok(cmdline)
}

/// Build the `m80.net.*` kernel command-line tokens for PID-1 network setup.
pub(crate) fn build_pid_one_network_cmdline(
    state: &VmNetworkStateRecord,
) -> Result<PidOneNetworkCmdline, NetError> {
    validate_ready_state(state)?;
    if state.dns_resolvers.is_empty() {
        return Err(NetError::NoUsableDnsResolvers);
    }
    Ok(PidOneNetworkCmdline {
        args: vec![
            "m80.net=outbound".to_owned(),
            "m80.net.iface=eth0".to_owned(),
            format!("m80.net.mac={}", state.guest_mac),
            format!(
                "m80.net.ipv4={}/{}",
                state.guest_ipv4,
                state.bridge.cidr.prefix_len()
            ),
            format!("m80.net.gateway={}", state.bridge.gateway_ipv4),
            format!(
                "m80.net.dns={}",
                state
                    .dns_resolvers
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ],
    })
}

/// Build the systemd-networkd and systemd-resolved file contents.
pub(crate) fn build_guest_network_config(
    state: &VmNetworkStateRecord,
    resolvers: &[Ipv4Addr],
) -> Result<GuestNetworkConfig, NetError> {
    if resolvers.is_empty() {
        return Err(NetError::NoUsableDnsResolvers);
    }
    let dns_lines = resolvers
        .iter()
        .map(|resolver| format!("DNS={resolver}"))
        .collect::<Vec<_>>()
        .join("\n");
    let resolver_list = resolvers
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");

    Ok(GuestNetworkConfig {
        networkd: format!(
            "[Match]\nMACAddress={}\n\n[Network]\nAddress={}/{}\nGateway={}\n{}\nIPv6AcceptRA=no\nLinkLocalAddressing=no\n",
            state.guest_mac,
            state.guest_ipv4,
            state.bridge.cidr.prefix_len(),
            state.bridge.gateway_ipv4,
            dns_lines,
        ),
        resolved: format!("[Resolve]\nDNS={resolver_list}\nFallbackDNS=\nDomains=~.\n"),
    })
}

fn validate_ready_state(state: &VmNetworkStateRecord) -> Result<(), NetError> {
    if state.setup_phase != SetupPhase::Ready {
        return Err(invalid_state(
            &state.run_dir,
            "VM network state must be ready before guest network injection",
        ));
    }
    if state.bridge.setup_phase != SetupPhase::Ready {
        return Err(invalid_state(
            &state.run_dir,
            "bridge state must be ready before guest network injection",
        ));
    }
    Ok(())
}

fn write_guest_network_config(
    ops: &mut impl GuestNetworkConfigOps,
    runtime_rootfs: &Path,
    config: &GuestNetworkConfig,
) -> Result<(), NetError> {
    ensure_ext4_dir(ops, runtime_rootfs, SYSTEMD_NETWORK_DIR)?;
    ensure_ext4_dir(ops, runtime_rootfs, SYSTEMD_RESOLVED_CONF_DIR)?;
    write_ext4_file(ops, runtime_rootfs, NETWORKD_FILE, &config.networkd)?;
    write_ext4_file(ops, runtime_rootfs, RESOLVED_FILE, &config.resolved)
}

fn ensure_ext4_dir(
    ops: &mut impl GuestNetworkConfigOps,
    image: &Path,
    image_dir: &str,
) -> Result<(), NetError> {
    let stat = ops.command_output(
        "debugfs",
        &[
            "-R".to_owned(),
            format!("stat {image_dir}"),
            image.display().to_string(),
        ],
    )?;
    if stat.status_success {
        return Ok(());
    }
    ops.run_command(
        "debugfs",
        &[
            "-w".to_owned(),
            "-R".to_owned(),
            format!("mkdir {image_dir}"),
            image.display().to_string(),
        ],
    )
}

fn write_ext4_file(
    ops: &mut impl GuestNetworkConfigOps,
    image: &Path,
    image_path: &str,
    content: &str,
) -> Result<(), NetError> {
    let mut temp = tempfile::NamedTempFile::new()?;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;

    let temp_path_str = temp
        .path()
        .to_str()
        .ok_or_else(|| invalid_state(temp.path(), "temp file path is not valid UTF-8"))?;
    if temp_path_str.chars().any(char::is_whitespace) {
        return Err(invalid_state(
            temp.path(),
            "temp file path contains whitespace; debugfs -R write splits on whitespace",
        ));
    }

    ops.run_command(
        "debugfs",
        &[
            "-w".to_owned(),
            "-R".to_owned(),
            format!("write {temp_path_str} {image_path}"),
            image.display().to_string(),
        ],
    )
}
