//! Bridge and TAP link operations, backed by rtnetlink and the Linux TUN/TAP
//! driver. The [`LinkOps`] trait is the test seam; [`NetlinkLinkOps`] is the
//! real host backend. The `ip` binary is not used at runtime.

use std::fs::File;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::stream::TryStreamExt;
use ipnet::Ipv4Net;
use nix::sched::CloneFlags;
use rtnetlink::{
    new_connection, packet_route::link::BridgeStpState, Handle, LinkBridge, LinkBridgePort,
    LinkUnspec, LinkVeth, NetworkNamespace,
};
use tokio::runtime::{Builder, Runtime};

use crate::{NetError, VmNetworkStateRecord};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PrivateNetnsTapPlan {
    pub(crate) bridge_name: String,
    pub(crate) tap_name: String,
    pub(crate) vmm_netns_name: String,
    pub(crate) vmm_netns_path: PathBuf,
    pub(crate) host_veth_name: String,
    pub(crate) vmm_veth_name: String,
    pub(crate) vmm_bridge_name: String,
    pub(crate) vmm_bridge_mac: [u8; 6],
    pub(crate) bridge_cidr: Ipv4Net,
    pub(crate) tap_link_mac: [u8; 6],
    pub(crate) tap_mtu: Option<u32>,
}

pub(crate) const MIN_TAP_MTU: u32 = 576;
pub(crate) const MAX_TAP_MTU: u32 = 9000;

/// Host link operations needed by bridge/TAP setup.
///
/// Production uses [`NetlinkLinkOps`]. Tests can provide a recording
/// implementation so state-machine behavior is verified without `CAP_NET_ADMIN`.
pub(crate) trait LinkOps {
    /// Create a Linux bridge interface.
    fn create_bridge(&mut self, name: &str) -> Result<(), NetError>;
    /// Add an IPv4 address to a link.
    fn add_ipv4_address(
        &mut self,
        link_name: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), NetError>;
    /// Create a TAP interface.
    fn create_tap(&mut self, name: &str) -> Result<(), NetError>;
    /// Set the link MAC address.
    fn set_link_mac(&mut self, name: &str, mac: [u8; 6]) -> Result<(), NetError>;
    /// Set a link MTU.
    fn set_link_mtu(&mut self, name: &str, mtu: u32) -> Result<(), NetError>;
    /// Attach a link to a bridge.
    fn attach_link_to_bridge(&mut self, link_name: &str, bridge_name: &str)
        -> Result<(), NetError>;
    /// Enable bridge-port isolation on a link already attached to a bridge.
    fn set_bridge_port_isolated(&mut self, link_name: &str) -> Result<(), NetError>;
    /// Bring a link up.
    fn set_link_up(&mut self, name: &str) -> Result<(), NetError>;
    /// Delete a link when it exists.
    fn delete_link_if_exists(&mut self, name: &str) -> Result<(), NetError>;
    /// Create a persistent named network namespace.
    fn create_network_namespace(&mut self, name: &str) -> Result<(), NetError>;
    /// Delete a persistent named network namespace when present.
    fn delete_network_namespace_if_exists(&mut self, name: &str) -> Result<(), NetError>;
    /// Create a veth pair in the host namespace.
    fn create_veth_pair(&mut self, host_name: &str, peer_name: &str) -> Result<(), NetError>;
    /// Move a link into the namespace identified by `netns_path`.
    fn move_link_to_namespace(
        &mut self,
        link_name: &str,
        netns_path: &Path,
    ) -> Result<(), NetError>;
    /// Create a bridge inside the namespace identified by `netns_path`.
    fn create_bridge_in_namespace(
        &mut self,
        netns_path: &Path,
        bridge_name: &str,
    ) -> Result<(), NetError>;
    /// Create a TAP inside the namespace identified by `netns_path`.
    fn create_tap_in_namespace(
        &mut self,
        netns_path: &Path,
        tap_name: &str,
    ) -> Result<(), NetError>;
    /// Set a link MAC inside the namespace identified by `netns_path`.
    fn set_link_mac_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
        mac: [u8; 6],
    ) -> Result<(), NetError>;
    /// Set a link MTU inside the namespace identified by `netns_path`.
    fn set_link_mtu_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
        mtu: u32,
    ) -> Result<(), NetError>;
    /// Attach a link to a bridge inside the namespace identified by `netns_path`.
    fn attach_link_to_bridge_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
        bridge_name: &str,
    ) -> Result<(), NetError>;
    /// Bring a link up inside the namespace identified by `netns_path`.
    fn set_link_up_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
    ) -> Result<(), NetError>;
    /// Return whether a link exists.
    fn link_exists(&mut self, name: &str) -> Result<bool, NetError>;
    /// Return whether a link has the given IPv4 address and prefix length.
    fn link_has_ipv4_address(
        &mut self,
        link_name: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<bool, NetError>;
}

pub(crate) fn teardown_tap(ops: &mut impl LinkOps, tap_name: &str) -> Result<(), NetError> {
    ops.delete_link_if_exists(tap_name)
}

pub(crate) fn create_private_netns_tap_topology(
    ops: &mut impl LinkOps,
    plan: &PrivateNetnsTapPlan,
) -> Result<(), NetError> {
    validate_tap_mtu(plan.tap_mtu)?;
    ops.create_network_namespace(&plan.vmm_netns_name)?;
    ops.create_veth_pair(&plan.host_veth_name, &plan.vmm_veth_name)?;
    ops.attach_link_to_bridge(&plan.host_veth_name, &plan.bridge_name)?;
    ops.set_link_up(&plan.host_veth_name)?;
    ops.move_link_to_namespace(&plan.vmm_veth_name, &plan.vmm_netns_path)?;
    ops.create_bridge_in_namespace(&plan.vmm_netns_path, &plan.vmm_bridge_name)?;
    ops.set_link_mac_in_namespace(
        &plan.vmm_netns_path,
        &plan.vmm_bridge_name,
        plan.vmm_bridge_mac,
    )?;
    ops.create_tap_in_namespace(&plan.vmm_netns_path, &plan.tap_name)?;
    ops.set_link_mac_in_namespace(&plan.vmm_netns_path, &plan.tap_name, plan.tap_link_mac)?;
    ops.attach_link_to_bridge_in_namespace(
        &plan.vmm_netns_path,
        &plan.tap_name,
        &plan.vmm_bridge_name,
    )?;
    ops.attach_link_to_bridge_in_namespace(
        &plan.vmm_netns_path,
        &plan.vmm_veth_name,
        &plan.vmm_bridge_name,
    )?;
    ops.set_bridge_port_isolated(&plan.host_veth_name)?;
    ops.set_link_up_in_namespace(&plan.vmm_netns_path, &plan.vmm_bridge_name)?;
    ops.set_link_up_in_namespace(&plan.vmm_netns_path, &plan.tap_name)?;
    if let Some(tap_mtu) = plan.tap_mtu {
        ops.set_link_mtu_in_namespace(&plan.vmm_netns_path, &plan.tap_name, tap_mtu)?;
    }
    ops.set_link_up_in_namespace(&plan.vmm_netns_path, &plan.vmm_veth_name)
}

fn validate_tap_mtu(tap_mtu: Option<u32>) -> Result<(), NetError> {
    match tap_mtu {
        Some(mtu) if !(MIN_TAP_MTU..=MAX_TAP_MTU).contains(&mtu) => Err(NetError::InvalidTapMtu {
            mtu,
            min: MIN_TAP_MTU,
            max: MAX_TAP_MTU,
        }),
        _ => Ok(()),
    }
}

pub(crate) fn teardown_private_netns_tap_topology(
    ops: &mut impl LinkOps,
    state: &VmNetworkStateRecord,
) -> Result<(), NetError> {
    ops.delete_link_if_exists(&state.host_veth_name)?;
    ops.delete_network_namespace_if_exists(&state.vmm_netns_name)
}

/// Real host backend for bridge and TAP link operations via rtnetlink and the TUN/TAP driver.
pub(crate) struct NetlinkLinkOps {
    runtime: Runtime,
    handle: Handle,
    tap_devices: Vec<(String, tun::Device)>,
    runtime_build_elapsed: Duration,
}

impl NetlinkLinkOps {
    /// Open an rtnetlink connection and return a backend ready for link operations.
    pub(crate) fn new() -> Result<Self, NetError> {
        let runtime_started = Instant::now();
        let runtime = Builder::new_current_thread().enable_io().build()?;
        let runtime_build_elapsed = runtime_started.elapsed();
        let _runtime_guard = runtime.enter();
        let (connection, handle, _) =
            new_connection().map_err(|source| NetError::NetlinkOperationFailed {
                operation: "open rtnetlink connection",
                detail: source.to_string(),
            })?;
        let _connection_task = runtime.spawn(connection);
        Ok(Self {
            runtime,
            handle,
            tap_devices: Vec::new(),
            runtime_build_elapsed,
        })
    }

    pub(crate) fn runtime_build_elapsed(&self) -> Duration {
        self.runtime_build_elapsed
    }

    fn block_on<F, T>(&self, operation: &'static str, future: F) -> Result<T, NetError>
    where
        F: Future<Output = Result<T, rtnetlink::Error>>,
    {
        self.runtime
            .block_on(future)
            .map_err(|source| NetError::NetlinkOperationFailed {
                operation,
                detail: source.to_string(),
            })
    }

    fn link_index(&self, name: &str) -> Result<Option<u32>, NetError> {
        self.runtime.block_on(async {
            let mut links = self
                .handle
                .link()
                .get()
                .match_name(name.to_owned())
                .execute();
            links
                .try_next()
                .await
                .map(|link| link.map(|link| link.header.index))
                .map_err(|source| NetError::NetlinkOperationFailed {
                    operation: "get link index",
                    detail: source.to_string(),
                })
        })
    }

    fn require_link_index(&self, name: &str, operation: &'static str) -> Result<u32, NetError> {
        self.link_index(name)?
            .ok_or_else(|| NetError::LinkNotFound {
                operation,
                name: name.to_owned(),
            })
    }

    fn drop_tap_handle(&mut self, name: &str) {
        self.tap_devices
            .retain(|(tap_name, _device)| tap_name != name);
    }

    fn run_in_namespace<F>(
        &self,
        netns_path: &Path,
        operation: &'static str,
        f: F,
    ) -> Result<(), NetError>
    where
        F: FnOnce(&mut NetlinkLinkOps) -> Result<(), NetError>,
    {
        let original = File::open("/proc/self/ns/net").map_err(|source| NetError::PathIo {
            path: PathBuf::from("/proc/self/ns/net"),
            source,
        })?;
        let target = File::open(netns_path).map_err(|source| NetError::PathIo {
            path: netns_path.to_path_buf(),
            source,
        })?;
        nix::sched::setns(&target, CloneFlags::CLONE_NEWNET).map_err(|source| {
            NetError::NetlinkOperationFailed {
                operation,
                detail: source.to_string(),
            }
        })?;
        let result = (|| {
            let mut namespaced = NetlinkLinkOps::new()?;
            f(&mut namespaced)
        })();
        let restore = nix::sched::setns(&original, CloneFlags::CLONE_NEWNET).map_err(|source| {
            NetError::NetlinkOperationFailed {
                operation: "restore host network namespace",
                detail: source.to_string(),
            }
        });
        result.and(restore)
    }
}

impl LinkOps for NetlinkLinkOps {
    fn create_bridge(&mut self, name: &str) -> Result<(), NetError> {
        self.block_on(
            "create bridge",
            self.handle
                .link()
                .add(
                    LinkBridge::new(name)
                        .stp_state(BridgeStpState::Disabled)
                        .forward_delay(0)
                        .build(),
                )
                .execute(),
        )
    }

    fn add_ipv4_address(
        &mut self,
        link_name: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<(), NetError> {
        let index = self.require_link_index(link_name, "add IPv4 address")?;
        self.block_on(
            "add IPv4 address",
            self.handle
                .address()
                .add(index, IpAddr::V4(address), prefix_len)
                .execute(),
        )
    }

    fn create_tap(&mut self, name: &str) -> Result<(), NetError> {
        let mut config = tun::configure();
        config
            .tun_name(name)
            .layer(tun::Layer::L2)
            .platform_config(|platform| {
                platform.ensure_root_privileges(false);
            });
        let mut device = tun::create(&config).map_err(|source| NetError::TapOperationFailed {
            operation: "create tap",
            source: source.into(),
        })?;
        device
            .persist()
            .map_err(|source| NetError::TapOperationFailed {
                operation: "make tap persistent",
                source: source.into(),
            })?;
        self.tap_devices.push((name.to_owned(), device));
        Ok(())
    }

    fn set_link_mac(&mut self, name: &str, mac: [u8; 6]) -> Result<(), NetError> {
        self.block_on(
            "set link MAC",
            self.handle
                .link()
                .set(
                    LinkUnspec::new_with_name(name)
                        .address(mac.to_vec())
                        .build(),
                )
                .execute(),
        )
    }

    fn set_link_mtu(&mut self, name: &str, mtu: u32) -> Result<(), NetError> {
        self.block_on(
            "set link MTU",
            self.handle
                .link()
                .set(LinkUnspec::new_with_name(name).mtu(mtu).build())
                .execute(),
        )
    }

    fn attach_link_to_bridge(
        &mut self,
        link_name: &str,
        bridge_name: &str,
    ) -> Result<(), NetError> {
        let bridge_index = self.require_link_index(bridge_name, "attach link to bridge")?;
        self.block_on(
            "attach link to bridge",
            self.handle
                .link()
                .set(
                    LinkUnspec::new_with_name(link_name)
                        .controller(bridge_index)
                        .build(),
                )
                .execute(),
        )
    }

    fn set_bridge_port_isolated(&mut self, link_name: &str) -> Result<(), NetError> {
        let port_index = self.require_link_index(link_name, "set bridge port isolation")?;
        self.block_on(
            "set bridge port isolation",
            self.handle
                .link()
                .set_port(LinkBridgePort::new(port_index).isolated(true).build())
                .execute(),
        )
    }

    fn set_link_up(&mut self, name: &str) -> Result<(), NetError> {
        self.block_on(
            "set link up",
            self.handle
                .link()
                .set(LinkUnspec::new_with_name(name).up().build())
                .execute(),
        )
    }

    fn delete_link_if_exists(&mut self, name: &str) -> Result<(), NetError> {
        self.drop_tap_handle(name);
        if let Some(index) = self.link_index(name)? {
            self.block_on("delete link", self.handle.link().del(index).execute())?;
        }
        Ok(())
    }

    fn create_network_namespace(&mut self, name: &str) -> Result<(), NetError> {
        self.block_on(
            "create network namespace",
            NetworkNamespace::add(name.to_owned()),
        )
    }

    fn delete_network_namespace_if_exists(&mut self, name: &str) -> Result<(), NetError> {
        let path = PathBuf::from(crate::OUTBOUND_NETNS_DIR).join(name);
        if !path.exists() {
            return Ok(());
        }
        self.block_on(
            "delete network namespace",
            NetworkNamespace::del(name.to_owned()),
        )
    }

    fn create_veth_pair(&mut self, host_name: &str, peer_name: &str) -> Result<(), NetError> {
        self.block_on(
            "create veth pair",
            self.handle
                .link()
                .add(LinkVeth::new(host_name, peer_name).build())
                .execute(),
        )
    }

    fn move_link_to_namespace(
        &mut self,
        link_name: &str,
        netns_path: &Path,
    ) -> Result<(), NetError> {
        let netns = File::open(netns_path).map_err(|source| NetError::PathIo {
            path: netns_path.to_path_buf(),
            source,
        })?;
        self.block_on(
            "move link to namespace",
            self.handle
                .link()
                .set(
                    LinkUnspec::new_with_name(link_name)
                        .setns_by_fd(netns.as_raw_fd())
                        .build(),
                )
                .execute(),
        )
    }

    fn create_bridge_in_namespace(
        &mut self,
        netns_path: &Path,
        bridge_name: &str,
    ) -> Result<(), NetError> {
        self.run_in_namespace(netns_path, "create bridge in namespace", |ops| {
            ops.create_bridge(bridge_name)
        })
    }

    fn create_tap_in_namespace(
        &mut self,
        netns_path: &Path,
        tap_name: &str,
    ) -> Result<(), NetError> {
        self.run_in_namespace(netns_path, "create tap in namespace", |ops| {
            ops.create_tap(tap_name)
        })
    }

    fn set_link_mac_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
        mac: [u8; 6],
    ) -> Result<(), NetError> {
        self.run_in_namespace(netns_path, "set link MAC in namespace", |ops| {
            ops.set_link_mac(link_name, mac)
        })
    }

    fn set_link_mtu_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
        mtu: u32,
    ) -> Result<(), NetError> {
        self.run_in_namespace(netns_path, "set link MTU in namespace", |ops| {
            ops.set_link_mtu(link_name, mtu)
        })
    }

    fn attach_link_to_bridge_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
        bridge_name: &str,
    ) -> Result<(), NetError> {
        self.run_in_namespace(netns_path, "attach link to bridge in namespace", |ops| {
            ops.attach_link_to_bridge(link_name, bridge_name)
        })
    }

    fn set_link_up_in_namespace(
        &mut self,
        netns_path: &Path,
        link_name: &str,
    ) -> Result<(), NetError> {
        self.run_in_namespace(netns_path, "set link up in namespace", |ops| {
            ops.set_link_up(link_name)
        })
    }

    fn link_exists(&mut self, name: &str) -> Result<bool, NetError> {
        Ok(self.link_index(name)?.is_some())
    }

    fn link_has_ipv4_address(
        &mut self,
        link_name: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> Result<bool, NetError> {
        let index = self.require_link_index(link_name, "verify IPv4 address")?;
        self.runtime.block_on(async {
            let mut addresses = self
                .handle
                .address()
                .get()
                .set_link_index_filter(index)
                .set_prefix_length_filter(prefix_len)
                .set_address_filter(IpAddr::V4(address))
                .execute();
            addresses
                .try_next()
                .await
                .map(|address| address.is_some())
                .map_err(|source| NetError::NetlinkOperationFailed {
                    operation: "verify IPv4 address",
                    detail: source.to_string(),
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_ops_source_contains_no_ip_shellout() {
        let source = include_str!("link_ops.rs");
        let command_new_ip = ["Command::new", "(\"ip\")"].join("");
        let command_new_sbin_ip = ["Command::new", "(\"", "/sbin", "/ip", "\")"].join("");
        let sbin_ip = ["/sbin", "/ip"].join("");
        let ip_tuntap = ["ip", " tuntap"].join("");

        assert!(!source.contains(&command_new_ip));
        assert!(!source.contains(&command_new_sbin_ip));
        assert!(!source.contains(&sbin_ip));
        assert!(!source.contains(&ip_tuntap));
    }

    #[test]
    #[ignore = "requires-root requires-network-namespace"]
    fn tap_creation_and_teardown_without_sbin_ip() {
        let suffix = std::process::id() & 0x00ff_ffff;
        let tap_name = format!("tfctest{suffix:06x}");
        let mut ops = NetlinkLinkOps::new().unwrap();

        ops.create_tap(&tap_name).unwrap();
        assert!(ops.link_index(&tap_name).unwrap().is_some());

        ops.delete_link_if_exists(&tap_name).unwrap();
        assert!(ops.link_index(&tap_name).unwrap().is_none());
    }
}
