//! PID-1 outbound network configuration for systemd-free images.

use std::fmt::Write as _;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use anyhow::Context as _;
use futures_util::stream::TryStreamExt;
use ipnet::Ipv4Net;
use rtnetlink::{new_connection, Handle, LinkUnspec, RouteMessageBuilder};
use tokio::runtime::{Builder, Runtime};

use crate::guest_log::{self, GuestLogPhase};

const NET_OUTBOUND: &str = "m80.net=outbound";
const NET_JOIN_NETNS: &str = "m80.net=join_netns";
const NET_IFACE_PREFIX: &str = "m80.net.iface=";
const NET_IPV4_PREFIX: &str = "m80.net.ipv4=";
const NET_GATEWAY_PREFIX: &str = "m80.net.gateway=";
const NET_DNS_PREFIX: &str = "m80.net.dns=";
const NET_MAC_PREFIX: &str = "m80.net.mac=";

/// Parsed PID-1 network configuration from the kernel command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PidOneNetworkConfig {
    /// Guest interface name, normally `eth0`.
    pub iface: String,
    /// Guest IPv4 address with prefix.
    pub ipv4: Ipv4Net,
    /// Default gateway.
    pub gateway: Ipv4Addr,
    /// DNS resolvers written to `/etc/resolv.conf`.
    pub dns: Vec<Ipv4Addr>,
    /// Guest MAC address assigned to the Firecracker virtio-net device.
    pub mac: String,
}

/// Read `/proc/cmdline` and apply outbound networking when requested.
pub(crate) fn configure_from_proc_cmdline() -> anyhow::Result<()> {
    let cmdline = std::fs::read_to_string("/proc/cmdline").context("read /proc/cmdline")?;
    let Some(config) = parse_cmdline_network_config(&cmdline)? else {
        guest_log::info(
            GuestLogPhase::Boot,
            None,
            "outbound network config not requested",
        );
        return Ok(());
    };
    configure_network(&config, &RealNetworkOps)
}

/// Parse m80 PID-1 outbound networking tokens from a kernel command line.
pub(crate) fn parse_cmdline_network_config(cmdline: &str) -> anyhow::Result<Option<PidOneNetworkConfig>> {
    if !cmdline
        .split_whitespace()
        .any(|token| token == NET_OUTBOUND || token == NET_JOIN_NETNS)
    {
        return Ok(None);
    }

    let iface = required_token(cmdline, NET_IFACE_PREFIX)?;
    let ipv4 = required_token(cmdline, NET_IPV4_PREFIX)?
        .parse::<Ipv4Net>()
        .context("parse m80.net.ipv4")?;
    let gateway = required_token(cmdline, NET_GATEWAY_PREFIX)?
        .parse::<Ipv4Addr>()
        .context("parse m80.net.gateway")?;
    let dns = parse_dns_list(&required_token(cmdline, NET_DNS_PREFIX)?)?;
    let mac = required_token(cmdline, NET_MAC_PREFIX)?;

    Ok(Some(PidOneNetworkConfig {
        iface,
        ipv4,
        gateway,
        dns,
        mac,
    }))
}

fn required_token(cmdline: &str, prefix: &str) -> anyhow::Result<String> {
    cmdline
        .split_whitespace()
        .find_map(|token| token.strip_prefix(prefix).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing required outbound network token {prefix}<value>"))
}

fn parse_dns_list(raw: &str) -> anyhow::Result<Vec<Ipv4Addr>> {
    let dns = raw
        .split(',')
        .map(|part| part.parse::<Ipv4Addr>().context("parse m80.net.dns"))
        .collect::<Result<Vec<_>, _>>()?;
    if dns.is_empty() {
        anyhow::bail!("m80.net.dns must include at least one resolver");
    }
    Ok(dns)
}

fn configure_network(config: &PidOneNetworkConfig, ops: &impl NetworkOps) -> anyhow::Result<()> {
    guest_log::info(
        GuestLogPhase::Boot,
        None,
        format!(
            "configuring outbound network {} {} via {}",
            config.iface, config.ipv4, config.gateway
        ),
    );
    ops.configure_link(config)?;
    ops.write_resolv_conf(&config.dns)?;
    Ok(())
}

trait NetworkOps {
    fn configure_link(&self, config: &PidOneNetworkConfig) -> anyhow::Result<()>;
    fn write_resolv_conf(&self, dns: &[Ipv4Addr]) -> anyhow::Result<()>;
}

struct RealNetworkOps;

impl NetworkOps for RealNetworkOps {
    fn configure_link(&self, config: &PidOneNetworkConfig) -> anyhow::Result<()> {
        RtnetlinkConfigurator::new()
            .context("open rtnetlink")?
            .configure(config)
    }

    fn write_resolv_conf(&self, dns: &[Ipv4Addr]) -> anyhow::Result<()> {
        write_resolv_conf(Path::new("/etc/resolv.conf"), dns)
    }
}

fn write_resolv_conf(path: &Path, dns: &[Ipv4Addr]) -> anyhow::Result<()> {
    let mut content = String::new();
    for resolver in dns {
        writeln!(&mut content, "nameserver {resolver}")?;
    }
    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

struct RtnetlinkConfigurator {
    runtime: Runtime,
    handle: Handle,
}

impl RtnetlinkConfigurator {
    fn new() -> anyhow::Result<Self> {
        let runtime = Builder::new_current_thread().enable_io().build()?;
        let _runtime_guard = runtime.enter();
        let (connection, handle, _) = new_connection()?;
        let _connection_task = runtime.spawn(connection);
        Ok(Self { runtime, handle })
    }

    fn configure(self, config: &PidOneNetworkConfig) -> anyhow::Result<()> {
        let index = self.link_index(&config.iface)?;
        self.block_on(
            "add IPv4 address",
            self.handle
                .address()
                .add(
                    index,
                    IpAddr::V4(config.ipv4.addr()),
                    config.ipv4.prefix_len(),
                )
                .execute(),
        )?;
        self.block_on(
            "set link up",
            self.handle
                .link()
                .set(LinkUnspec::new_with_name(&config.iface).up().build())
                .execute(),
        )?;
        let route = RouteMessageBuilder::<Ipv4Addr>::new()
            .destination_prefix(Ipv4Addr::UNSPECIFIED, 0)
            .gateway(config.gateway)
            .output_interface(index)
            .build();
        self.block_on(
            "add default route",
            self.handle.route().add(route).execute(),
        )
    }

    fn block_on<F, T>(&self, operation: &'static str, future: F) -> anyhow::Result<T>
    where
        F: Future<Output = Result<T, rtnetlink::Error>>,
    {
        self.runtime.block_on(future).with_context(|| operation)
    }

    fn link_index(&self, iface: &str) -> anyhow::Result<u32> {
        self.runtime.block_on(async {
            let mut links = self
                .handle
                .link()
                .get()
                .match_name(iface.to_owned())
                .execute();
            links
                .try_next()
                .await
                .with_context(|| format!("get link {iface}"))?
                .map(|link| link.header.index)
                .ok_or_else(|| anyhow::anyhow!("network interface {iface} not found"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingOps {
        configured: std::cell::RefCell<Vec<PidOneNetworkConfig>>,
        resolv_conf: std::cell::RefCell<Vec<Vec<Ipv4Addr>>>,
    }

    impl NetworkOps for RecordingOps {
        fn configure_link(&self, config: &PidOneNetworkConfig) -> anyhow::Result<()> {
            self.configured.borrow_mut().push(config.clone());
            Ok(())
        }

        fn write_resolv_conf(&self, dns: &[Ipv4Addr]) -> anyhow::Result<()> {
            self.resolv_conf.borrow_mut().push(dns.to_vec());
            Ok(())
        }
    }

    #[test]
    fn absent_network_flag_skips_configuration() {
        assert_eq!(
            parse_cmdline_network_config("console=ttyS0 m80.workspace=0").unwrap(),
            None
        );
    }

    #[test]
    fn parses_outbound_cmdline_tokens() {
        let config = parse_cmdline_network_config(
            "console=ttyS0 m80.net=outbound m80.net.iface=eth0 \
             m80.net.ipv4=172.16.4.2/24 m80.net.gateway=172.16.4.1 \
             m80.net.dns=1.1.1.1,8.8.8.8 m80.net.mac=02:00:00:00:00:02",
        )
        .unwrap()
        .unwrap();

        assert_eq!(config.iface, "eth0");
        assert_eq!(config.ipv4, "172.16.4.2/24".parse::<Ipv4Net>().unwrap());
        assert_eq!(config.gateway, Ipv4Addr::new(172, 16, 4, 1));
        assert_eq!(
            config.dns,
            vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)]
        );
        assert_eq!(config.mac, "02:00:00:00:00:02");
    }

    #[test]
    fn parses_join_netns_cmdline_tokens() {
        let config = parse_cmdline_network_config(
            "console=ttyS0 m80.net=join_netns m80.net.iface=eth0 \
             m80.net.ipv4=10.80.0.2/24 m80.net.gateway=10.80.0.1 \
             m80.net.dns=10.80.0.1 m80.net.mac=02:00:00:00:80:01",
        )
        .unwrap()
        .unwrap();

        assert_eq!(config.iface, "eth0");
        assert_eq!(config.ipv4, "10.80.0.2/24".parse::<Ipv4Net>().unwrap());
        assert_eq!(config.gateway, Ipv4Addr::new(10, 80, 0, 1));
        assert_eq!(config.dns, vec![Ipv4Addr::new(10, 80, 0, 1)]);
        assert_eq!(config.mac, "02:00:00:00:80:01");
    }

    #[test]
    fn enabled_network_requires_all_tokens() {
        let err = parse_cmdline_network_config("m80.net=outbound m80.net.iface=eth0")
            .unwrap_err()
            .to_string();
        assert!(err.contains("m80.net.ipv4"), "{err}");
    }

    #[test]
    fn configure_network_applies_link_then_dns() {
        let config = parse_cmdline_network_config(
            "m80.net=outbound m80.net.iface=eth0 m80.net.ipv4=172.16.4.2/24 \
             m80.net.gateway=172.16.4.1 m80.net.dns=1.1.1.1 \
             m80.net.mac=02:00:00:00:00:02",
        )
        .unwrap()
        .unwrap();
        let ops = RecordingOps::default();

        configure_network(&config, &ops).unwrap();

        assert_eq!(
            ops.configured.borrow().as_slice(),
            std::slice::from_ref(&config)
        );
        assert_eq!(
            ops.resolv_conf.borrow().as_slice(),
            &[vec![Ipv4Addr::new(1, 1, 1, 1)]]
        );
    }

    #[test]
    fn writes_resolv_conf_nameserver_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resolv.conf");

        write_resolv_conf(
            &path,
            &[Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "nameserver 1.1.1.1\nnameserver 8.8.8.8\n"
        );
    }
}
