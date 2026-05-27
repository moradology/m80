use std::path::{Path, PathBuf};

use m80_preflight::{Discovery, LaunchPath};

use crate::error::NetworkHelperError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NetworkHelperLaunch {
    Direct {
        helper_bin: PathBuf,
    },
    Systemd {
        systemd_run_bin: PathBuf,
        helper_bin: PathBuf,
    },
}

impl NetworkHelperLaunch {
    pub(super) fn from_discovery(discovery: &Discovery) -> Result<Self, NetworkHelperError> {
        match discovery.chosen_launch_path {
            LaunchPath::Wrapper => Ok(Self::direct(discovery.net_helper_bin.clone())),
            LaunchPath::Systemd => {
                let systemd_run_bin = discovery.systemd_run_bin.clone().ok_or_else(|| {
                    NetworkHelperError::SystemdLaunchConfig {
                        reason: "preflight selected systemd but did not record systemd_run_bin"
                            .to_owned(),
                    }
                })?;
                Ok(Self::Systemd {
                    systemd_run_bin,
                    helper_bin: discovery.net_helper_bin.clone(),
                })
            }
        }
    }

    pub(super) fn direct(helper_bin: PathBuf) -> Self {
        Self::Direct { helper_bin }
    }

    pub(super) fn helper_bin(&self) -> &Path {
        match self {
            Self::Direct { helper_bin } | Self::Systemd { helper_bin, .. } => helper_bin,
        }
    }

    pub(super) fn descriptor(&self) -> String {
        match self {
            Self::Direct { helper_bin } => format!("direct:{}", helper_bin.display()),
            Self::Systemd {
                systemd_run_bin,
                helper_bin,
            } => format!(
                "systemd:{}:{}",
                systemd_run_bin.display(),
                helper_bin.display()
            ),
        }
    }
}
