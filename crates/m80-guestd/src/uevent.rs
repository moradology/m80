//! Guest-side Linux uevent parsing, matching, and bounded wait primitives.

#![allow(dead_code)] // m80-iswt.4 lands infrastructure before mount handlers use it.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use nix::sys::socket::{
    bind, recv, socket, AddressFamily, MsgFlags, NetlinkAddr, SockFlag, SockProtocol, SockType,
};

const UEVENT_GROUP_KERNEL: u32 = 1;
const UEVENT_BUFFER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Uevent {
    pub(crate) action: String,
    pub(crate) devpath: String,
    pub(crate) subsystem: Option<String>,
    pub(crate) devname: Option<String>,
    pub(crate) properties: HashMap<String, String>,
}

impl Uevent {
    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }
}

pub(crate) trait UeventMatcher: Send + Sync {
    fn matches(&self, event: &Uevent) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockDeviceMatcher {
    devname: Option<String>,
}

impl BlockDeviceMatcher {
    pub(crate) fn any() -> Self {
        Self { devname: None }
    }

    pub(crate) fn devname(devname: impl Into<String>) -> Self {
        Self {
            devname: Some(devname.into()),
        }
    }
}

impl UeventMatcher for BlockDeviceMatcher {
    fn matches(&self, event: &Uevent) -> bool {
        if event.action != "add" && event.action != "change" {
            return false;
        }
        if event.subsystem.as_deref() != Some("block") {
            return false;
        }
        match &self.devname {
            Some(devname) => event.devname.as_deref() == Some(devname.as_str()),
            None => true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum UeventWaitError {
    #[error("timed out waiting for device uevent")]
    Timeout,
}

#[derive(Default)]
pub(crate) struct UeventRegistry {
    state: Mutex<UeventState>,
    changed: Condvar,
}

#[derive(Default)]
struct UeventState {
    cache: Vec<Uevent>,
}

impl UeventRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(&self, event: Uevent) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.cache.push(event);
        self.changed.notify_all();
    }

    pub(crate) fn wait_for<M: UeventMatcher>(
        &self,
        matcher: &M,
        timeout: Duration,
    ) -> Result<Uevent, UeventWaitError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(event) = state.cache.iter().find(|event| matcher.matches(event)) {
                return Ok(event.clone());
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(UeventWaitError::Timeout);
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_state, wait) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
            state = next_state;
            if wait.timed_out() {
                return Err(UeventWaitError::Timeout);
            }
        }
    }
}

pub(crate) fn parse_uevent(bytes: &[u8]) -> Option<Uevent> {
    let mut fields = bytes
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| std::str::from_utf8(part).ok());

    let header = fields.next()??;
    let (action, devpath) = header.split_once('@')?;
    let mut properties = HashMap::new();
    for field in fields.flatten() {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        properties.insert(key.to_owned(), value.to_owned());
    }

    Some(Uevent {
        action: action.to_owned(),
        devpath: devpath.to_owned(),
        subsystem: properties.get("SUBSYSTEM").cloned(),
        devname: properties.get("DEVNAME").cloned(),
        properties,
    })
}

pub(crate) fn spawn_netlink_listener(
    registry: Arc<UeventRegistry>,
) -> anyhow::Result<thread::JoinHandle<anyhow::Result<()>>> {
    let fd = socket(
        AddressFamily::Netlink,
        SockType::Raw,
        SockFlag::SOCK_CLOEXEC,
        SockProtocol::NetlinkKObjectUEvent,
    )
    .context("open NETLINK_KOBJECT_UEVENT socket")?;
    bind(fd.as_raw_fd(), &NetlinkAddr::new(0, UEVENT_GROUP_KERNEL))
        .context("bind NETLINK_KOBJECT_UEVENT socket")?;

    Ok(thread::spawn(move || {
        let mut buf = vec![0u8; UEVENT_BUFFER_BYTES];
        loop {
            let n = recv(fd.as_raw_fd(), &mut buf, MsgFlags::empty())
                .context("recv NETLINK_KOBJECT_UEVENT message")?;
            if let Some(event) = parse_uevent(&buf[..n]) {
                registry.record(event);
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_event(devname: &str) -> Uevent {
        parse_uevent(
            format!(
                "add@/devices/pci0000:00/virtio0/block/{devname}\0ACTION=add\0SUBSYSTEM=block\0DEVNAME={devname}\0SEQNUM=7\0"
            )
            .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn parser_reads_header_and_properties() {
        let event = block_event("vdc");

        assert_eq!(event.action, "add");
        assert!(event.devpath.ends_with("/block/vdc"));
        assert_eq!(event.subsystem.as_deref(), Some("block"));
        assert_eq!(event.devname.as_deref(), Some("vdc"));
        assert_eq!(event.property("SEQNUM"), Some("7"));
    }

    #[test]
    fn block_matcher_filters_subsystem_action_and_devname() {
        let event = block_event("vdc");
        assert!(BlockDeviceMatcher::any().matches(&event));
        assert!(BlockDeviceMatcher::devname("vdc").matches(&event));
        assert!(!BlockDeviceMatcher::devname("vdd").matches(&event));

        let net_event = parse_uevent(
            b"add@/devices/virtual/net/eth0\0ACTION=add\0SUBSYSTEM=net\0DEVNAME=eth0\0",
        )
        .unwrap();
        assert!(!BlockDeviceMatcher::any().matches(&net_event));
    }

    #[test]
    fn wait_for_returns_cached_matching_event() {
        let registry = UeventRegistry::new();
        registry.record(block_event("vdc"));

        let event = registry
            .wait_for(&BlockDeviceMatcher::devname("vdc"), Duration::from_secs(1))
            .unwrap();

        assert_eq!(event.devname.as_deref(), Some("vdc"));
    }

    #[test]
    fn wait_for_blocks_until_event_arrives_without_polling() {
        let registry = Arc::new(UeventRegistry::new());
        let sender = Arc::clone(&registry);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            sender.record(block_event("vdd"));
        });

        let event = registry
            .wait_for(
                &BlockDeviceMatcher::devname("vdd"),
                Duration::from_millis(250),
            )
            .unwrap();

        assert_eq!(event.devname.as_deref(), Some("vdd"));
    }

    #[test]
    fn wait_for_times_out_without_matching_event() {
        let registry = UeventRegistry::new();
        registry.record(block_event("vdc"));

        let err = registry
            .wait_for(
                &BlockDeviceMatcher::devname("vdd"),
                Duration::from_millis(1),
            )
            .unwrap_err();

        assert!(matches!(err, UeventWaitError::Timeout));
    }
}
