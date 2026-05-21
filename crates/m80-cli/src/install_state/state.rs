use super::{
    ActivePointerReport, ActivePointerStatus, InstallConfigReport, InstallProfileReport,
    InstallStateDiagnostic, InstallStateDiagnosticCode, InstallStateKind,
};

pub(super) fn classify_install_state(
    active: &ActivePointerReport,
    config: &InstallConfigReport,
    profile: Option<&InstallProfileReport>,
    diagnostics: &[InstallStateDiagnostic],
) -> InstallStateKind {
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            InstallStateDiagnosticCode::ConfigLoadFailed
                | InstallStateDiagnosticCode::DefaultProfileMissing
                | InstallStateDiagnosticCode::ProfileLoadFailed
                | InstallStateDiagnosticCode::ActivePointerTraversal
                | InstallStateDiagnosticCode::ActivePointerOutsideInstallRoot
                | InstallStateDiagnosticCode::ActivePointerNotVersionDir
                | InstallStateDiagnosticCode::ProfilePathTraversal
                | InstallStateDiagnosticCode::ProfilePathOutsideInstallRoot
                | InstallStateDiagnosticCode::ProfileArtifactDirMalformed
                | InstallStateDiagnosticCode::InstallMetadataInvalid
                | InstallStateDiagnosticCode::ProofCacheInvalid
        )
    }) {
        return InstallStateKind::InvalidInstallMetadata;
    }
    if config.explicit_override {
        return InstallStateKind::ExplicitOverride;
    }
    if profile.is_some_and(|profile| profile.body_source == "builtin_env") {
        return InstallStateKind::LocalDevTree;
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::ProofCacheStale)
    {
        return InstallStateKind::TamperedProofCache;
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == InstallStateDiagnosticCode::InstallMetadataStale)
    {
        return InstallStateKind::StaleInstallMetadata;
    }
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            InstallStateDiagnosticCode::InstallMetadataMissing
                | InstallStateDiagnosticCode::ProofCacheMissing
        )
    }) {
        return InstallStateKind::MissingInstallMetadata;
    }
    match active.status {
        ActivePointerStatus::Missing => InstallStateKind::MissingActivePointer,
        ActivePointerStatus::Dangling => InstallStateKind::DanglingActivePointer,
        ActivePointerStatus::Invalid => InstallStateKind::InvalidInstallMetadata,
        ActivePointerStatus::Live => {
            if let (Some(active_version), Some(profile)) = (active.version_dir.as_deref(), profile)
            {
                if profile.version_dir.as_deref() != Some(active_version) {
                    return InstallStateKind::StaleProfileTarget;
                }
            }
            InstallStateKind::HealthyActiveRelease
        }
    }
}

pub(super) fn should_read_metadata(
    active: &ActivePointerReport,
    profile: Option<&InstallProfileReport>,
) -> bool {
    let (ActivePointerStatus::Live, Some(active_version), Some(profile)) =
        (active.status, active.version_dir.as_deref(), profile)
    else {
        return false;
    };
    profile.body_source != "builtin_env" && profile.version_dir.as_deref() == Some(active_version)
}
