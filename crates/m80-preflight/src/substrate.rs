use std::fs;
use std::io;
use std::path::Path;

use caps::{CapSet, CapsHashSet};
use nix::sys::utsname::uname;
use nix::unistd::{geteuid, Gid, Group, Uid, User};

use crate::{
    classify_privilege, CheckRow, HostPrerequisiteCheck, HostPrerequisiteResult, PreflightError,
    PrivilegeStatus, REQUIRED_CAPABILITIES,
};

const KVM_PATH: &str = "/dev/kvm";
const ENV_JAIL_UID: &str = "M80_JAIL_UID";
const ENV_JAIL_GID: &str = "M80_JAIL_GID";
const ENV_MAX_CONCURRENT_VMS: &str = "M80_MAX_CONCURRENT_VMS";
const DEFAULT_JAIL_UID: u32 = 3000;
const DEFAULT_JAIL_GID: u32 = 3000;
const DEFAULT_EXPECTED_CONCURRENT_VMS: u32 = 8;
const MIN_HOST_KERNEL_MAJOR: u64 = 6;
const MIN_HOST_KERNEL_MINOR: u64 = 1;

/// Cgroup mode preflight should validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupPreflightMode {
    /// Caller will not use m80's cgroup v2 subtree support.
    Disabled,
    /// Caller will use m80's cgroup v2 subtree support.
    UnifiedV2,
}

/// Host feature knobs whose required checks depend on effective config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostFeaturePreflightConfig {
    /// Cgroup mode to validate.
    pub cgroup_mode: CgroupPreflightMode,
    /// UID that the official jailer will switch Firecracker to.
    pub jail_uid: u32,
    /// GID that the official jailer will switch Firecracker to.
    pub jail_gid: u32,
    /// Expected concurrent VM count used to size host-global conntrack
    /// capacity.
    pub expected_concurrent_vms: u32,
}

impl HostFeaturePreflightConfig {
    /// Build from exact m80 env vars. Absent `M80_CGROUP_MODE` defaults to
    /// `unified-v2`, matching `m80-firecracker` config defaults.
    pub fn from_env() -> Result<Self, PreflightError> {
        let cgroup_mode = match std::env::var("M80_CGROUP_MODE") {
            Ok(value) => parse_cgroup_mode(&value)?,
            Err(std::env::VarError::NotPresent) => CgroupPreflightMode::UnifiedV2,
            Err(std::env::VarError::NotUnicode(value)) => {
                return Err(PreflightError::InvalidCgroupMode {
                    actual: value.to_string_lossy().into_owned(),
                });
            }
        };
        let jail_uid = parse_jail_id_env(ENV_JAIL_UID, DEFAULT_JAIL_UID)?;
        let jail_gid = parse_jail_id_env(ENV_JAIL_GID, DEFAULT_JAIL_GID)?;
        let expected_concurrent_vms = parse_expected_concurrent_vms_env()?;
        Ok(Self {
            cgroup_mode,
            jail_uid,
            jail_gid,
            expected_concurrent_vms,
        })
    }
}

/// What kind of proof produced a [`HostSubstrateDiscovery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSubstrateProofKind {
    /// Live host preflight probes ran against the current machine. This is
    /// still not a VM launch smoke.
    LivePreflight,
    /// A hostless fixture exercised the substrate verifier without touching
    /// `/dev/kvm`, host cgroups, passwd/group databases, or capabilities.
    HostlessFixture,
}

/// Non-mutating host-substrate verifier output for install and preflight.
#[derive(Debug, Clone)]
pub struct HostSubstrateDiscovery {
    /// Whether this was a live preflight or a hostless fixture proof.
    pub proof_kind: HostSubstrateProofKind,
    /// How m80 satisfies the privilege precondition on this host.
    pub privilege: PrivilegeStatus,
    /// Check rows produced by the substrate verifier.
    pub report: Vec<CheckRow>,
    /// Versioned machine-readable prerequisite result for the substrate rows.
    pub host_prerequisites: HostPrerequisiteResult,
}

/// KVM state exposed by a hostless substrate fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSubstrateFixtureKvm {
    /// `/dev/kvm` exists and opens for write.
    Writable,
    /// `/dev/kvm` is absent.
    Missing,
    /// `/dev/kvm` exists but write-open returns permission denied.
    NotWritable,
}

/// Hostless fixture inputs for [`verify_host_substrate_fixture`].
#[derive(Debug, Clone)]
pub struct HostSubstrateFixture {
    /// `uname -s` value to classify.
    pub sysname: String,
    /// `uname -r` value to classify.
    pub kernel_release: String,
    /// Simulated `/dev/kvm` state.
    pub kvm: HostSubstrateFixtureKvm,
    /// Whether `m80-cgroup::Subtree::probe()` should succeed.
    pub cgroup_v2_available: bool,
    /// Simulated passwd lookup for the configured jail UID.
    pub jail_user: Option<String>,
    /// Simulated group lookup for the configured jail GID.
    pub jail_group: Option<String>,
    /// Simulated effective uid.
    pub euid: u32,
    /// Simulated effective capability set.
    pub effective_caps: CapsHashSet,
}

impl HostSubstrateFixture {
    /// Build a passing root fixture that does not touch host `/dev/kvm`,
    /// cgroups, passwd/group databases, or real capability state.
    #[must_use]
    pub fn supported_root() -> Self {
        Self {
            sysname: "Linux".to_string(),
            kernel_release: "6.1.0".to_string(),
            kvm: HostSubstrateFixtureKvm::Writable,
            cgroup_v2_available: true,
            jail_user: Some("m80".to_string()),
            jail_group: Some("m80".to_string()),
            euid: 0,
            effective_caps: CapsHashSet::new(),
        }
    }
}

trait HostSubstrateProbe {
    fn uname(&self) -> Result<(String, String), PreflightError>;
    fn kvm_exists(&self) -> bool;
    fn open_kvm_for_write(&self) -> Result<(), io::Error>;
    fn cgroup_probe(&self) -> Result<(), m80_cgroup::CgroupError>;
    fn user_name(&self, uid: u32) -> Result<Option<String>, PreflightError>;
    fn group_name(&self, gid: u32) -> Result<Option<String>, PreflightError>;
    fn effective_privilege(&self) -> Result<(u32, CapsHashSet), PreflightError>;
}

struct LiveHostSubstrateProbe;

impl HostSubstrateProbe for LiveHostSubstrateProbe {
    fn uname(&self) -> Result<(String, String), PreflightError> {
        let uts = uname().map_err(|e| PreflightError::SystemIo {
            operation: "uname",
            source: e.into(),
        })?;
        Ok((
            uts.sysname().to_string_lossy().into_owned(),
            uts.release().to_string_lossy().into_owned(),
        ))
    }

    fn kvm_exists(&self) -> bool {
        Path::new(KVM_PATH).exists()
    }

    fn open_kvm_for_write(&self) -> Result<(), io::Error> {
        fs::OpenOptions::new().write(true).open(KVM_PATH).map(drop)
    }

    fn cgroup_probe(&self) -> Result<(), m80_cgroup::CgroupError> {
        m80_cgroup::Subtree::probe()
    }

    fn user_name(&self, uid: u32) -> Result<Option<String>, PreflightError> {
        User::from_uid(Uid::from_raw(uid))
            .map_err(|source| PreflightError::SystemIo {
                operation: "user lookup",
                source: source.into(),
            })
            .map(|user| user.map(|user| user.name))
    }

    fn group_name(&self, gid: u32) -> Result<Option<String>, PreflightError> {
        Group::from_gid(Gid::from_raw(gid))
            .map_err(|source| PreflightError::SystemIo {
                operation: "group lookup",
                source: source.into(),
            })
            .map(|group| group.map(|group| group.name))
    }

    fn effective_privilege(&self) -> Result<(u32, CapsHashSet), PreflightError> {
        let effective =
            caps::read(None, CapSet::Effective).map_err(PreflightError::CapabilityRead)?;
        Ok((geteuid().as_raw(), effective))
    }
}

impl HostSubstrateProbe for HostSubstrateFixture {
    fn uname(&self) -> Result<(String, String), PreflightError> {
        Ok((self.sysname.clone(), self.kernel_release.clone()))
    }

    fn kvm_exists(&self) -> bool {
        self.kvm != HostSubstrateFixtureKvm::Missing
    }

    fn open_kvm_for_write(&self) -> Result<(), io::Error> {
        match self.kvm {
            HostSubstrateFixtureKvm::Writable => Ok(()),
            HostSubstrateFixtureKvm::Missing => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "fixture /dev/kvm missing",
            )),
            HostSubstrateFixtureKvm::NotWritable => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture /dev/kvm permission denied",
            )),
        }
    }

    fn cgroup_probe(&self) -> Result<(), m80_cgroup::CgroupError> {
        if self.cgroup_v2_available {
            Ok(())
        } else {
            Err(m80_cgroup::CgroupError::UnsupportedHostMode)
        }
    }

    fn user_name(&self, _uid: u32) -> Result<Option<String>, PreflightError> {
        Ok(self.jail_user.clone())
    }

    fn group_name(&self, _gid: u32) -> Result<Option<String>, PreflightError> {
        Ok(self.jail_group.clone())
    }

    fn effective_privilege(&self) -> Result<(u32, CapsHashSet), PreflightError> {
        Ok((self.euid, self.effective_caps.clone()))
    }
}

/// Verify the non-mutating host substrate checks used before install
/// finalization and before launch.
pub fn verify_host_substrate(
    config: HostFeaturePreflightConfig,
) -> Result<HostSubstrateDiscovery, PreflightError> {
    verify_host_substrate_with_probe(
        &LiveHostSubstrateProbe,
        config,
        HostSubstrateProofKind::LivePreflight,
    )
}

/// Verify host substrate behavior against a hostless fixture.
///
/// This is for release/install fixture CI. A successful result is marked
/// [`HostSubstrateProofKind::HostlessFixture`] and is not a real-KVM run-smoke
/// proof.
pub fn verify_host_substrate_fixture(
    config: HostFeaturePreflightConfig,
    fixture: &HostSubstrateFixture,
) -> Result<HostSubstrateDiscovery, PreflightError> {
    verify_host_substrate_with_probe(fixture, config, HostSubstrateProofKind::HostlessFixture)
}

fn verify_host_substrate_with_probe<P: HostSubstrateProbe>(
    probe: &P,
    config: HostFeaturePreflightConfig,
    proof_kind: HostSubstrateProofKind,
) -> Result<HostSubstrateDiscovery, PreflightError> {
    let mut report = Vec::new();
    let (sysname, release) = probe.uname()?;
    check_os_values(&sysname, &release, &mut report)?;
    check_host_kernel_release(&release, &mut report)?;
    check_kvm_with_probe(probe, &mut report)?;
    check_cgroup_mode_with_probe(config.cgroup_mode, &mut report, || probe.cgroup_probe())?;
    let user = probe.user_name(config.jail_uid)?;
    let group = probe.group_name(config.jail_gid)?;
    report.push(classify_jailer_identity(
        config.jail_uid,
        config.jail_gid,
        user,
        group,
    )?);
    let (euid, effective) = probe.effective_privilege()?;
    let privilege = classify_privilege(euid, &effective)?;
    report.push(privilege_row(privilege));
    report.push(proof_kind_row(proof_kind));
    debug_assert!(report.iter().all(|row| row.passed));
    let host_prerequisites = HostPrerequisiteResult::new(
        report
            .iter()
            .map(|row| HostPrerequisiteCheck::pass(row.label.clone()))
            .collect(),
    );
    Ok(HostSubstrateDiscovery {
        proof_kind,
        privilege,
        report,
        host_prerequisites,
    })
}

fn check_os_values(
    sysname: &str,
    release: &str,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    if sysname != "Linux" {
        return Err(PreflightError::UnsupportedHostPlatform {
            actual: sysname.to_owned(),
        });
    }
    report.push(CheckRow {
        label: "OS gate".to_string(),
        passed: true,
        detail: format!("Linux {release}"),
    });
    Ok(())
}

fn check_host_kernel_release(
    release: &str,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    classify_host_kernel_release(release)?;
    report.push(CheckRow {
        label: "Host kernel floor".to_string(),
        passed: true,
        detail: format!("Linux {release} >= 6.1"),
    });
    Ok(())
}

pub(crate) fn classify_host_kernel_release(release: &str) -> Result<(), PreflightError> {
    let (major, minor) =
        parse_kernel_major_minor(release).ok_or_else(|| PreflightError::HostKernelUnsupported {
            actual: release.to_owned(),
            minimum: minimum_host_kernel_string(),
        })?;
    if (major, minor) < (MIN_HOST_KERNEL_MAJOR, MIN_HOST_KERNEL_MINOR) {
        return Err(PreflightError::HostKernelUnsupported {
            actual: release.to_owned(),
            minimum: minimum_host_kernel_string(),
        });
    }
    Ok(())
}

fn parse_kernel_major_minor(release: &str) -> Option<(u64, u64)> {
    let mut parts = release.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn minimum_host_kernel_string() -> String {
    format!("{MIN_HOST_KERNEL_MAJOR}.{MIN_HOST_KERNEL_MINOR}")
}

fn check_kvm_with_probe<P: HostSubstrateProbe>(
    probe: &P,
    report: &mut Vec<CheckRow>,
) -> Result<(), PreflightError> {
    let kvm = Path::new(KVM_PATH);
    classify_kvm_access(kvm, probe.kvm_exists(), probe.open_kvm_for_write())?;
    report.push(CheckRow {
        label: "KVM".to_string(),
        passed: true,
        detail: format!("{} present and writable", kvm.display()),
    });
    Ok(())
}

pub(crate) fn classify_kvm_access(
    path: &Path,
    exists: bool,
    write_open: Result<(), io::Error>,
) -> Result<(), PreflightError> {
    if !exists {
        return Err(PreflightError::KvmUnavailable {
            path: path.to_path_buf(),
        });
    }

    match write_open {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(PreflightError::KvmNotWritable {
                path: path.to_path_buf(),
            })
        }
        Err(source) => Err(PreflightError::PathIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn check_cgroup_mode_with_probe<F>(
    mode: CgroupPreflightMode,
    report: &mut Vec<CheckRow>,
    probe: F,
) -> Result<(), PreflightError>
where
    F: FnOnce() -> Result<(), m80_cgroup::CgroupError>,
{
    if mode == CgroupPreflightMode::UnifiedV2 {
        classify_cgroup_probe(mode, probe())?;
    }
    report.push(CheckRow {
        label: "Cgroup mode".to_string(),
        passed: true,
        detail: match mode {
            CgroupPreflightMode::Disabled => "disabled".to_string(),
            CgroupPreflightMode::UnifiedV2 => "unified-v2 available".to_string(),
        },
    });
    Ok(())
}

pub(crate) fn classify_cgroup_probe(
    mode: CgroupPreflightMode,
    probe: Result<(), m80_cgroup::CgroupError>,
) -> Result<(), PreflightError> {
    if mode == CgroupPreflightMode::Disabled {
        return Ok(());
    }

    match probe {
        Ok(()) => Ok(()),
        Err(m80_cgroup::CgroupError::UnsupportedHostMode) => {
            Err(PreflightError::CgroupV2Unavailable)
        }
        Err(err) => Err(PreflightError::SystemIo {
            operation: "cgroup v2 probe",
            source: std::io::Error::other(err),
        }),
    }
}

pub(crate) fn classify_jailer_identity(
    jail_uid: u32,
    jail_gid: u32,
    user: Option<String>,
    group: Option<String>,
) -> Result<CheckRow, PreflightError> {
    let user = user.ok_or(PreflightError::JailIdentityUnavailable {
        field: "jail_uid",
        id: jail_uid,
    })?;
    let group = group.ok_or(PreflightError::JailIdentityUnavailable {
        field: "jail_gid",
        id: jail_gid,
    })?;

    Ok(CheckRow {
        label: "Jailer identity".to_string(),
        passed: true,
        detail: format!("uid={jail_uid} ({user}), gid={jail_gid} ({group})"),
    })
}

fn privilege_row(status: PrivilegeStatus) -> CheckRow {
    CheckRow {
        label: "Privilege".to_string(),
        passed: true,
        detail: match status {
            PrivilegeStatus::Root => "euid == 0 (root)".to_string(),
            PrivilegeStatus::CapabilityBearing => format!(
                "capability-bearing ({})",
                REQUIRED_CAPABILITIES
                    .iter()
                    .map(|c| format!("{c:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    }
}

fn proof_kind_row(kind: HostSubstrateProofKind) -> CheckRow {
    CheckRow {
        label: "Host substrate proof".to_string(),
        passed: true,
        detail: match kind {
            HostSubstrateProofKind::LivePreflight => {
                "live preflight only; not a real-KVM run-smoke proof".to_string()
            }
            HostSubstrateProofKind::HostlessFixture => {
                "hostless fixture only; not a real-KVM run-smoke proof".to_string()
            }
        },
    }
}

fn parse_expected_concurrent_vms_env() -> Result<u32, PreflightError> {
    match std::env::var(ENV_MAX_CONCURRENT_VMS) {
        Ok(value) => parse_expected_concurrent_vms(&value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_EXPECTED_CONCURRENT_VMS),
        Err(std::env::VarError::NotUnicode(value)) => {
            Err(PreflightError::InvalidExpectedConcurrentVms {
                actual: value.to_string_lossy().into_owned(),
            })
        }
    }
}

fn parse_expected_concurrent_vms(value: &str) -> Result<u32, PreflightError> {
    match value.parse::<u32>() {
        Ok(0) | Err(_) => Err(PreflightError::InvalidExpectedConcurrentVms {
            actual: value.to_owned(),
        }),
        Ok(parsed) => Ok(parsed),
    }
}

fn parse_jail_id_env(env_key: &'static str, default: u32) -> Result<u32, PreflightError> {
    match std::env::var(env_key) {
        Ok(value) => parse_jail_id(env_key, &value),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(std::env::VarError::NotUnicode(value)) => Err(PreflightError::InvalidJailIdentity {
            field: env_key,
            value: value.to_string_lossy().into_owned(),
        }),
    }
}

pub(crate) fn parse_jail_id(field: &'static str, value: &str) -> Result<u32, PreflightError> {
    value
        .parse()
        .map_err(|_| PreflightError::InvalidJailIdentity {
            field,
            value: value.to_owned(),
        })
}

pub(crate) fn parse_cgroup_mode(value: &str) -> Result<CgroupPreflightMode, PreflightError> {
    match value {
        "disabled" => Ok(CgroupPreflightMode::Disabled),
        "unified-v2" => Ok(CgroupPreflightMode::UnifiedV2),
        other => Err(PreflightError::InvalidCgroupMode {
            actual: other.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> HostFeaturePreflightConfig {
        HostFeaturePreflightConfig {
            cgroup_mode: CgroupPreflightMode::UnifiedV2,
            jail_uid: 3000,
            jail_gid: 3000,
            expected_concurrent_vms: 8,
        }
    }

    fn verify_fixture(
        fixture: &HostSubstrateFixture,
    ) -> Result<HostSubstrateDiscovery, PreflightError> {
        verify_host_substrate_fixture(default_config(), fixture)
    }

    #[test]
    fn hostless_fixture_success_is_not_real_kvm_smoke() {
        let discovery = verify_fixture(&HostSubstrateFixture::supported_root()).unwrap();

        assert_eq!(
            discovery.proof_kind,
            HostSubstrateProofKind::HostlessFixture
        );
        assert_eq!(discovery.privilege, PrivilegeStatus::Root);
        let proof = discovery
            .report
            .iter()
            .find(|row| row.label == "Host substrate proof")
            .expect("proof row");
        assert!(proof.detail.contains("hostless fixture only"));
        assert!(proof.detail.contains("not a real-KVM run-smoke proof"));
    }

    #[test]
    fn substrate_fixture_rejects_unsupported_host() {
        let fixture = HostSubstrateFixture {
            sysname: "Darwin".to_string(),
            ..HostSubstrateFixture::supported_root()
        };

        let err = verify_fixture(&fixture).unwrap_err();

        assert!(matches!(
            err,
            PreflightError::UnsupportedHostPlatform { actual } if actual == "Darwin"
        ));
    }

    #[test]
    fn substrate_fixture_rejects_missing_kvm() {
        let fixture = HostSubstrateFixture {
            kvm: HostSubstrateFixtureKvm::Missing,
            ..HostSubstrateFixture::supported_root()
        };

        let err = verify_fixture(&fixture).unwrap_err();

        assert!(matches!(err, PreflightError::KvmUnavailable { .. }));
    }

    #[test]
    fn substrate_fixture_rejects_bad_kvm_permissions() {
        let fixture = HostSubstrateFixture {
            kvm: HostSubstrateFixtureKvm::NotWritable,
            ..HostSubstrateFixture::supported_root()
        };

        let err = verify_fixture(&fixture).unwrap_err();

        assert!(matches!(err, PreflightError::KvmNotWritable { .. }));
    }

    #[test]
    fn substrate_fixture_rejects_wrong_cgroup_mode() {
        let fixture = HostSubstrateFixture {
            cgroup_v2_available: false,
            ..HostSubstrateFixture::supported_root()
        };

        let err = verify_fixture(&fixture).unwrap_err();

        assert!(matches!(err, PreflightError::CgroupV2Unavailable));
    }

    #[test]
    fn substrate_fixture_can_model_hostless_preflight_without_cgroup_probe() {
        let fixture = HostSubstrateFixture {
            cgroup_v2_available: false,
            ..HostSubstrateFixture::supported_root()
        };
        let mut config = default_config();
        config.cgroup_mode = CgroupPreflightMode::Disabled;

        let discovery = verify_host_substrate_fixture(config, &fixture).unwrap();

        assert!(discovery
            .report
            .iter()
            .any(|row| { row.label == "Cgroup mode" && row.detail == "disabled" }));
    }

    #[test]
    fn substrate_fixture_rejects_missing_jail_user() {
        let fixture = HostSubstrateFixture {
            jail_user: None,
            ..HostSubstrateFixture::supported_root()
        };

        let err = verify_fixture(&fixture).unwrap_err();

        assert!(matches!(
            err,
            PreflightError::JailIdentityUnavailable {
                field: "jail_uid",
                id: 3000
            }
        ));
    }

    #[test]
    fn substrate_fixture_rejects_insufficient_privilege() {
        let fixture = HostSubstrateFixture {
            euid: 1000,
            effective_caps: CapsHashSet::new(),
            ..HostSubstrateFixture::supported_root()
        };

        let err = verify_fixture(&fixture).unwrap_err();

        assert!(matches!(
            err,
            PreflightError::PrivilegeUnavailable { missing_caps } if !missing_caps.is_empty()
        ));
    }

    #[test]
    fn substrate_failures_carry_operator_remediation_hints() {
        let unsupported_host = PreflightError::UnsupportedHostPlatform {
            actual: "Darwin".to_string(),
        };
        assert!(unsupported_host.hint().contains("Linux"));

        let old_kernel = PreflightError::HostKernelUnsupported {
            actual: "5.15".to_string(),
            minimum: "6.1".to_string(),
        };
        assert!(old_kernel.hint().contains("6.1"));

        let kvm = PreflightError::KvmUnavailable {
            path: Path::new("/dev/kvm").to_path_buf(),
        };
        assert!(kvm.hint().contains("/dev/kvm"));
        assert!(kvm.hint().contains("KVM"));

        let kvm_perms = PreflightError::KvmNotWritable {
            path: Path::new("/dev/kvm").to_path_buf(),
        };
        assert!(kvm_perms.hint().contains("kvm"));
        assert!(kvm_perms.hint().contains("root"));

        let cgroup = PreflightError::CgroupV2Unavailable;
        assert!(cgroup.hint().contains("unified cgroup v2"));

        let jail = PreflightError::JailIdentityUnavailable {
            field: "jail_uid",
            id: 3000,
        };
        assert!(jail.hint().contains("M80_JAIL_UID"));
        assert!(jail.hint().contains("M80_JAIL_GID"));

        let privilege = PreflightError::PrivilegeUnavailable {
            missing_caps: REQUIRED_CAPABILITIES.to_vec(),
        };
        assert!(privilege.hint().contains("setcap"));
        assert!(privilege
            .hint()
            .contains("securityContext.capabilities.add"));
    }
}
