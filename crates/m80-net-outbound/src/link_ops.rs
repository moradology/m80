//! Bridge and TAP link operations, backed by rtnetlink and the Linux TUN/TAP
//! driver. The [`LinkOps`] trait is the test seam; [`NetlinkLinkOps`] is the
//! real host backend. The `ip` binary is not used at runtime.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use futures_util::stream::TryStreamExt;
use ipnet::Ipv4Net;
use rtnetlink::{new_connection, Handle, LinkBridge, LinkUnspec};
use tokio::runtime::{Builder, Runtime};

use crate::NetError;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TapBridgePlan {
    pub(crate) bridge_name: String,
    pub(crate) tap_name: String,
    pub(crate) bridge_cidr: Ipv4Net,
    pub(crate) guest_mac: [u8; 6],
}

/// Host link operations needed by bridge/TAP setup.
///
/// Production uses [`NetlinkLinkOps`]. Tests can provide a recording
/// implementation so state-machine behavior is verified without `CAP_NET_ADMIN`.
pub trait LinkOps {
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
    /// Attach a link to a bridge.
    fn attach_link_to_bridge(&mut self, link_name: &str, bridge_name: &str)
        -> Result<(), NetError>;
    /// Bring a link up.
    fn set_link_up(&mut self, name: &str) -> Result<(), NetError>;
    /// Delete a link when it exists.
    fn delete_link_if_exists(&mut self, name: &str) -> Result<(), NetError>;
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

/// Create a full bridge+tap stack in one call. Used in tests; production callers
/// use [`create_tap_on_bridge`] after the bridge is already up.
#[allow(dead_code)]
pub(crate) fn create_tap_bridge(
    ops: &mut impl LinkOps,
    plan: &TapBridgePlan,
) -> Result<(), NetError> {
    let gateway = plan
        .bridge_cidr
        .hosts()
        .next()
        .ok_or_else(|| NetError::InvalidNetworkState {
            path: plan.bridge_name.clone().into(),
            detail: format!("{} has no gateway host address", plan.bridge_cidr),
        })?;

    ops.create_bridge(&plan.bridge_name)?;
    ops.add_ipv4_address(&plan.bridge_name, gateway, plan.bridge_cidr.prefix_len())?;
    ops.set_link_up(&plan.bridge_name)?;
    ops.create_tap(&plan.tap_name)?;
    ops.set_link_mac(&plan.tap_name, plan.guest_mac)?;
    ops.attach_link_to_bridge(&plan.tap_name, &plan.bridge_name)?;
    ops.set_link_up(&plan.tap_name)
}

pub(crate) fn teardown_tap(ops: &mut impl LinkOps, tap_name: &str) -> Result<(), NetError> {
    ops.delete_link_if_exists(tap_name)
}

pub(crate) fn create_tap_on_bridge(
    ops: &mut impl LinkOps,
    plan: &TapBridgePlan,
) -> Result<(), NetError> {
    ops.create_tap(&plan.tap_name)?;
    ops.set_link_mac(&plan.tap_name, plan.guest_mac)?;
    ops.attach_link_to_bridge(&plan.tap_name, &plan.bridge_name)?;
    ops.set_link_up(&plan.tap_name)
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
}

impl LinkOps for NetlinkLinkOps {
    fn create_bridge(&mut self, name: &str) -> Result<(), NetError> {
        self.block_on(
            "create bridge",
            self.handle
                .link()
                .add(LinkBridge::new(name).build())
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
    fn tap_bridge_lifecycle_orders_link_operations() {
        let mut ops = RecordingLinkOps::default();
        let plan = TapBridgePlan {
            bridge_name: "brfc12345678901".to_owned(),
            tap_name: "tfc123456789012".to_owned(),
            bridge_cidr: "172.29.86.0/24".parse().unwrap(),
            guest_mac: [0x02, 0x8d, 0x9d, 0x42, 0xdb, 0xfd],
        };

        create_tap_bridge(&mut ops, &plan).unwrap();
        teardown_tap(&mut ops, &plan.tap_name).unwrap();

        assert_eq!(
            ops.operations,
            [
                "create_bridge brfc12345678901",
                "add_ipv4_address brfc12345678901 172.29.86.1/24",
                "set_link_up brfc12345678901",
                "create_tap tfc123456789012",
                "set_link_mac tfc123456789012 02:8d:9d:42:db:fd",
                "attach_link_to_bridge tfc123456789012 brfc12345678901",
                "set_link_up tfc123456789012",
                "delete_link_if_exists tfc123456789012",
            ]
        );
    }

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
    #[ignore = "requires CAP_NET_ADMIN and creates a host TAP interface"]
    fn tap_creation_and_teardown_without_sbin_ip() {
        let suffix = std::process::id() & 0x00ff_ffff;
        let tap_name = format!("tfctest{suffix:06x}");
        let mut ops = NetlinkLinkOps::new().unwrap();

        ops.create_tap(&tap_name).unwrap();
        assert!(ops.link_index(&tap_name).unwrap().is_some());

        ops.delete_link_if_exists(&tap_name).unwrap();
        assert!(ops.link_index(&tap_name).unwrap().is_none());
    }

    #[derive(Default)]
    struct RecordingLinkOps {
        operations: Vec<String>,
    }

    impl LinkOps for RecordingLinkOps {
        fn create_bridge(&mut self, name: &str) -> Result<(), NetError> {
            self.operations.push(format!("create_bridge {name}"));
            Ok(())
        }

        fn add_ipv4_address(
            &mut self,
            link_name: &str,
            address: Ipv4Addr,
            prefix_len: u8,
        ) -> Result<(), NetError> {
            self.operations.push(format!(
                "add_ipv4_address {link_name} {address}/{prefix_len}"
            ));
            Ok(())
        }

        fn create_tap(&mut self, name: &str) -> Result<(), NetError> {
            self.operations.push(format!("create_tap {name}"));
            Ok(())
        }

        fn set_link_mac(&mut self, name: &str, mac: [u8; 6]) -> Result<(), NetError> {
            self.operations.push(format!(
                "set_link_mac {name} {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            ));
            Ok(())
        }

        fn attach_link_to_bridge(
            &mut self,
            link_name: &str,
            bridge_name: &str,
        ) -> Result<(), NetError> {
            self.operations
                .push(format!("attach_link_to_bridge {link_name} {bridge_name}"));
            Ok(())
        }

        fn set_link_up(&mut self, name: &str) -> Result<(), NetError> {
            self.operations.push(format!("set_link_up {name}"));
            Ok(())
        }

        fn delete_link_if_exists(&mut self, name: &str) -> Result<(), NetError> {
            self.operations
                .push(format!("delete_link_if_exists {name}"));
            Ok(())
        }

        fn link_exists(&mut self, name: &str) -> Result<bool, NetError> {
            self.operations.push(format!("link_exists {name}"));
            Ok(true)
        }

        fn link_has_ipv4_address(
            &mut self,
            link_name: &str,
            address: Ipv4Addr,
            prefix_len: u8,
        ) -> Result<bool, NetError> {
            self.operations.push(format!(
                "link_has_ipv4_address {link_name} {address}/{prefix_len}"
            ));
            Ok(true)
        }
    }
}
