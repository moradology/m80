use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use m80_firecracker::{FcError, WarmPoolSnapshot};

use crate::args::EgressMode;
use crate::config;

pub(super) const WARM_SOCKET: &str = "owner.sock";
pub(super) const WARM_IDENTITY: &str = "owner.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmStatus {
    pub owner: WarmOwner,
    pub profile: WarmProfile,
    pub slots: WarmSlots,
    pub lifecycle: WarmLifecycle,
    pub last_error: Option<WarmStatusError>,
    pub paths: WarmPaths,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmOwner {
    pub state: String,
    pub mode: Option<String>,
    pub identity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmProfile {
    pub requested: String,
    pub active: Option<String>,
    pub compatible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmSlots {
    pub target_ready: usize,
    pub ready: usize,
    pub filling: usize,
    pub leased: usize,
    pub discarded: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmLifecycle {
    pub accepting_leases: bool,
    pub draining: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmStatusError {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmPaths {
    pub run_root: String,
    pub socket: Option<String>,
    pub log: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct WarmOwnerIdentity {
    pub binary_version: String,
    pub profile: String,
    pub egress: String,
    pub target_ready: usize,
    pub pid: u32,
    pub mode: String,
    pub socket_path: String,
    pub started_at_unix_ms: u64,
}

pub(super) fn warm_root() -> Result<PathBuf, FcError> {
    config::resolve_run_root().map(|root| root.join("warm"))
}

pub(super) fn socket_path() -> Result<PathBuf, FcError> {
    Ok(warm_root()?.join(WARM_SOCKET))
}

pub(super) fn identity_path() -> Result<PathBuf, FcError> {
    Ok(warm_root()?.join(WARM_IDENTITY))
}

pub(super) fn snapshot_dir() -> Result<PathBuf, FcError> {
    Ok(warm_root()?.join("snapshot"))
}

pub(super) fn unavailable(profile: Option<String>) -> WarmStatus {
    let root = warm_root()
        .unwrap_or_else(|_| PathBuf::from("/var/run/m80").join("warm"))
        .display()
        .to_string();
    WarmStatus {
        owner: WarmOwner {
            state: "unavailable".to_owned(),
            mode: None,
            identity: None,
        },
        profile: WarmProfile {
            requested: requested_profile(profile),
            active: None,
            compatible: false,
        },
        slots: WarmSlots::empty(),
        lifecycle: WarmLifecycle {
            accepting_leases: false,
            draining: false,
        },
        last_error: Some(WarmStatusError {
            kind: "owner_unavailable".to_owned(),
            detail: "no resident warm owner is running".to_owned(),
        }),
        paths: WarmPaths {
            run_root: root,
            socket: socket_path().ok().map(|p| p.display().to_string()),
            log: None,
        },
    }
}

pub(super) fn disabled(profile: Option<String>) -> WarmStatus {
    let root = warm_root()
        .unwrap_or_else(|_| PathBuf::from("/var/run/m80").join("warm"))
        .display()
        .to_string();
    WarmStatus {
        owner: WarmOwner {
            state: "disabled".to_owned(),
            mode: None,
            identity: None,
        },
        profile: WarmProfile {
            requested: requested_profile(profile),
            active: None,
            compatible: false,
        },
        slots: WarmSlots::empty(),
        lifecycle: WarmLifecycle {
            accepting_leases: false,
            draining: false,
        },
        last_error: None,
        paths: WarmPaths {
            run_root: root,
            socket: socket_path().ok().map(|p| p.display().to_string()),
            log: None,
        },
    }
}

pub(super) fn available(
    identity: &WarmOwnerIdentity,
    requested_profile: Option<String>,
    snapshot: WarmPoolSnapshot,
    accepting_leases: bool,
    draining: bool,
    last_error: Option<WarmStatusError>,
) -> WarmStatus {
    let requested = requested_profile.unwrap_or_else(|| identity.profile.clone());
    let compatible = requested == identity.profile;
    WarmStatus {
        owner: WarmOwner {
            state: if draining { "draining" } else { "available" }.to_owned(),
            mode: Some(identity.mode.clone()),
            identity: Some(format!("pid:{}", identity.pid)),
        },
        profile: WarmProfile {
            requested,
            active: Some(identity.profile.clone()),
            compatible,
        },
        slots: WarmSlots {
            target_ready: snapshot.target_ready,
            ready: snapshot.ready,
            filling: snapshot.filling,
            leased: snapshot.leased,
            discarded: snapshot.discarded,
        },
        lifecycle: WarmLifecycle {
            accepting_leases,
            draining,
        },
        last_error,
        paths: WarmPaths {
            run_root: warm_root()
                .unwrap_or_else(|_| PathBuf::from("/var/run/m80").join("warm"))
                .display()
                .to_string(),
            socket: Some(identity.socket_path.clone()),
            log: None,
        },
    }
}

impl WarmSlots {
    fn empty() -> Self {
        Self {
            target_ready: 0,
            ready: 0,
            filling: 0,
            leased: 0,
            discarded: 0,
        }
    }
}

pub(super) fn requested_profile(profile: Option<String>) -> String {
    profile.unwrap_or_else(|| "default".to_owned())
}

pub(super) fn egress_label(egress: EgressMode) -> &'static str {
    match egress {
        EgressMode::None => "none",
        EgressMode::Outbound => "outbound",
    }
}

pub(super) fn write_identity(identity: &WarmOwnerIdentity) -> Result<(), FcError> {
    fs::create_dir_all(warm_root()?).map_err(FcError::Io)?;
    fs::write(
        identity_path()?,
        serde_json::to_vec_pretty(identity).map_err(|e| FcError::Json {
            context: "serialize warm owner identity",
            source: e,
        })?,
    )
    .map_err(FcError::Io)
}

pub(super) fn remove_owner_state() -> Result<(), FcError> {
    let root = warm_root()?;
    match fs::remove_file(root.join(WARM_SOCKET)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(FcError::Io(e)),
    }
    match fs::remove_file(root.join(WARM_IDENTITY)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(FcError::Io(e)),
    }
    Ok(())
}

pub(super) fn format_human(status: &WarmStatus) -> String {
    let mut out = String::new();
    out.push_str(&format!("owner: {}\n", status.owner.state));
    if let Some(identity) = &status.owner.identity {
        out.push_str(&format!("identity: {identity}\n"));
    }
    out.push_str(&format!(
        "profile: requested={} active={} compatible={}\n",
        status.profile.requested,
        status.profile.active.as_deref().unwrap_or("-"),
        status.profile.compatible
    ));
    out.push_str(&format!(
        "slots: target_ready={} ready={} filling={} leased={} discarded={}\n",
        status.slots.target_ready,
        status.slots.ready,
        status.slots.filling,
        status.slots.leased,
        status.slots.discarded
    ));
    out.push_str(&format!(
        "lifecycle: accepting_leases={} draining={}\n",
        status.lifecycle.accepting_leases, status.lifecycle.draining
    ));
    if let Some(err) = &status.last_error {
        out.push_str(&format!("last_error: {}: {}\n", err.kind, err.detail));
    }
    out.push_str(&format!("run_root: {}\n", status.paths.run_root));
    if let Some(socket) = &status.paths.socket {
        out.push_str(&format!("socket: {socket}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use m80_firecracker::WarmPoolSnapshot;

    #[test]
    fn unavailable_owner_status_has_json_fields() {
        let status = unavailable(Some("default".to_owned()));

        assert_eq!(status.owner.state, "unavailable");
        assert_eq!(status.profile.requested, "default");
        assert_eq!(status.slots.ready, 0);
        assert_eq!(
            status.last_error.as_ref().map(|e| e.kind.as_str()),
            Some("owner_unavailable")
        );
    }

    #[test]
    fn incompatible_profile_status_fails_closed() {
        let identity = WarmOwnerIdentity {
            binary_version: "0.0.0".to_owned(),
            profile: "ubuntu".to_owned(),
            egress: "outbound".to_owned(),
            target_ready: 2,
            pid: 4242,
            mode: "foreground".to_owned(),
            socket_path: "/run/m80/warm/owner.sock".to_owned(),
            started_at_unix_ms: 0,
        };
        let status = available(
            &identity,
            Some("minimal".to_owned()),
            WarmPoolSnapshot {
                target_ready: 2,
                ready: 0,
                filling: 0,
                leased: 0,
                discarded: 2,
            },
            false,
            false,
            Some(WarmStatusError {
                kind: "profile_mismatch".to_owned(),
                detail: "requested profile identity does not match the owner".to_owned(),
            }),
        );

        assert_eq!(status.owner.state, "available");
        assert_eq!(status.profile.requested, "minimal");
        assert_eq!(status.profile.active.as_deref(), Some("ubuntu"));
        assert!(!status.profile.compatible);
        assert!(!status.lifecycle.accepting_leases);
        assert_eq!(
            status.last_error.as_ref().map(|e| e.kind.as_str()),
            Some("profile_mismatch")
        );
    }

    #[test]
    fn drain_and_disable_transitions_are_distinct() {
        let identity = WarmOwnerIdentity {
            binary_version: "0.0.0".to_owned(),
            profile: "minimal".to_owned(),
            egress: "outbound".to_owned(),
            target_ready: 2,
            pid: 4242,
            mode: "foreground".to_owned(),
            socket_path: "/run/m80/warm/owner.sock".to_owned(),
            started_at_unix_ms: 0,
        };
        let draining = available(
            &identity,
            Some("minimal".to_owned()),
            WarmPoolSnapshot {
                target_ready: 2,
                ready: 0,
                filling: 0,
                leased: 1,
                discarded: 1,
            },
            false,
            true,
            None,
        );
        let disabled = disabled(Some("minimal".to_owned()));

        assert_eq!(draining.owner.state, "draining");
        assert!(draining.lifecycle.draining);
        assert_eq!(draining.slots.leased, 1);
        assert_eq!(disabled.owner.state, "disabled");
        assert!(!disabled.lifecycle.draining);
        assert_eq!(disabled.slots.leased, 0);
    }
}
