//! Parent-process capability hardening after privileged helpers start.

#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(test))]
use std::sync::Mutex;

#[cfg(not(test))]
use caps::{CapSet, Capability};

use crate::error::CapabilityDropError;

const CAP_NET_ADMIN_BIT: u32 = 12;
#[cfg(not(test))]
static CAP_NET_ADMIN_DROPPED: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
static CAP_NET_ADMIN_DROP_LOCK: Mutex<()> = Mutex::new(());

/// Drop `CAP_NET_ADMIN` from the parent process once per process.
#[cfg(not(test))]
pub(crate) fn drop_parent_cap_net_admin() -> Result<(), CapabilityDropError> {
    if CAP_NET_ADMIN_DROPPED.load(Ordering::Acquire) {
        return Ok(());
    }
    let _guard = CAP_NET_ADMIN_DROP_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if CAP_NET_ADMIN_DROPPED.load(Ordering::Acquire) {
        return Ok(());
    }

    let before = read_parent_capability_status()?;
    if !before.has_cap_net_admin() {
        CAP_NET_ADMIN_DROPPED.store(true, Ordering::Release);
        return Ok(());
    }

    drop_cap_from_bounding()?;
    drop_cap_from_set(CapSet::Ambient, "ambient")?;
    drop_cap_from_set(CapSet::Inheritable, "inheritable")?;
    drop_cap_from_set(CapSet::Effective, "effective")?;
    drop_cap_from_set(CapSet::Permitted, "permitted")?;

    let after = read_parent_capability_status()?;
    after.verify_cap_net_admin_absent()?;
    CAP_NET_ADMIN_DROPPED.store(true, Ordering::Release);
    Ok(())
}

#[cfg(not(test))]
fn drop_cap_from_bounding() -> Result<(), CapabilityDropError> {
    let current =
        caps::read(None, CapSet::Bounding).map_err(|source| CapabilityDropError::ReadCaps {
            set: "bounding",
            source,
        })?;
    if current.contains(&Capability::CAP_NET_ADMIN) {
        caps::drop(None, CapSet::Bounding, Capability::CAP_NET_ADMIN)
            .map_err(|source| CapabilityDropError::DropBounding { source })?;
    }
    Ok(())
}

#[cfg(not(test))]
fn drop_cap_from_set(set: CapSet, set_name: &'static str) -> Result<(), CapabilityDropError> {
    let mut current = caps::read(None, set).map_err(|source| CapabilityDropError::ReadCaps {
        set: set_name,
        source,
    })?;
    if current.remove(&Capability::CAP_NET_ADMIN) {
        caps::set(None, set, &current).map_err(|source| CapabilityDropError::SetCaps {
            set: set_name,
            source,
        })?;
    }
    Ok(())
}

/// Parsed `/proc/self/status` capability fields used by tests and verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParentCapabilityStatus {
    cap_eff: u64,
    cap_prm: u64,
    cap_bnd: u64,
}

impl ParentCapabilityStatus {
    fn has_cap_net_admin(self) -> bool {
        self.cap_eff & cap_net_admin_mask() != 0
            || self.cap_prm & cap_net_admin_mask() != 0
            || self.cap_bnd & cap_net_admin_mask() != 0
    }

    fn verify_cap_net_admin_absent(self) -> Result<(), CapabilityDropError> {
        for (field, value) in [
            ("CapEff", self.cap_eff),
            ("CapPrm", self.cap_prm),
            ("CapBnd", self.cap_bnd),
        ] {
            if value & cap_net_admin_mask() != 0 {
                return Err(CapabilityDropError::Verification { field });
            }
        }
        Ok(())
    }
}

#[cfg(not(test))]
fn read_parent_capability_status() -> Result<ParentCapabilityStatus, CapabilityDropError> {
    let text = std::fs::read_to_string("/proc/self/status")
        .map_err(|source| CapabilityDropError::StatusRead { source })?;
    parse_parent_capability_status(&text)
}

pub(crate) fn parse_parent_capability_status(
    text: &str,
) -> Result<ParentCapabilityStatus, CapabilityDropError> {
    Ok(ParentCapabilityStatus {
        cap_eff: parse_status_capability_field(text, "CapEff")?,
        cap_prm: parse_status_capability_field(text, "CapPrm")?,
        cap_bnd: parse_status_capability_field(text, "CapBnd")?,
    })
}

fn parse_status_capability_field(
    text: &str,
    field: &'static str,
) -> Result<u64, CapabilityDropError> {
    let Some(raw) = text
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{field}:\t")))
    else {
        return Err(CapabilityDropError::StatusParse {
            field,
            value: "missing".to_owned(),
        });
    };
    u64::from_str_radix(raw.trim(), 16).map_err(|_| CapabilityDropError::StatusParse {
        field,
        value: raw.to_owned(),
    })
}

fn cap_net_admin_mask() -> u64 {
    1u64 << CAP_NET_ADMIN_BIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_capability_status_fields() {
        let status = parse_parent_capability_status(
            "Name:\tm80\nCapEff:\t0000000000001000\nCapPrm:\t0000000000000000\nCapBnd:\t0000000000001000\n",
        )
        .unwrap();

        assert!(status.has_cap_net_admin());
    }

    #[test]
    fn verifies_cap_net_admin_absent() {
        let status = parse_parent_capability_status(
            "CapEff:\t0000000000000000\nCapPrm:\t0000000000000000\nCapBnd:\t0000000000000000\n",
        )
        .unwrap();

        status.verify_cap_net_admin_absent().unwrap();
    }

    #[test]
    fn verification_names_remaining_capability_field() {
        let status = parse_parent_capability_status(
            "CapEff:\t0000000000000000\nCapPrm:\t0000000000001000\nCapBnd:\t0000000000000000\n",
        )
        .unwrap();

        let err = status.verify_cap_net_admin_absent().unwrap_err();

        assert!(matches!(
            err,
            CapabilityDropError::Verification { field: "CapPrm" }
        ));
    }
}
