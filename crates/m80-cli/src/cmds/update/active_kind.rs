use serde::Serialize;

use crate::install_state::{InstallStateKind, InstallStateReport};
use crate::release_policy::{classify_release_tag, ReleaseIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ActiveInstallKind {
    StableRelease,
    Prerelease,
    Ineligible,
    LocalDev,
    MissingActive,
    StaleActiveMetadata,
}

pub(super) fn active_install_kind(
    report: &InstallStateReport,
    active_tag: Option<&str>,
) -> ActiveInstallKind {
    match report.state {
        InstallStateKind::LocalDevTree => ActiveInstallKind::LocalDev,
        InstallStateKind::MissingActivePointer | InstallStateKind::DanglingActivePointer => {
            ActiveInstallKind::MissingActive
        }
        InstallStateKind::HealthyActiveRelease => match active_tag.map(classify_release_tag) {
            Some(ReleaseIdentity::Stable(_)) => ActiveInstallKind::StableRelease,
            Some(ReleaseIdentity::Prerelease { .. }) => ActiveInstallKind::Prerelease,
            Some(
                ReleaseIdentity::BuildMetadata { .. }
                | ReleaseIdentity::Malformed { .. }
                | ReleaseIdentity::LocalDev,
            ) => ActiveInstallKind::Ineligible,
            None => ActiveInstallKind::MissingActive,
        },
        InstallStateKind::ExplicitOverride => ActiveInstallKind::Ineligible,
        InstallStateKind::StaleProfileTarget
        | InstallStateKind::MissingInstallMetadata
        | InstallStateKind::StaleInstallMetadata
        | InstallStateKind::TamperedProofCache
        | InstallStateKind::InvalidInstallMetadata => ActiveInstallKind::StaleActiveMetadata,
    }
}
