use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

const NET_HELPER_UNIT_PREFIX: &str = "m80-net-helper";
const NET_HELPER_CAPABILITY_BOUNDING_SET: &[&str] = &["CAP_NET_ADMIN", "CAP_SYS_ADMIN"];

static NET_HELPER_UNIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NetworkHelperSystemdInvocation {
    pub(super) program: PathBuf,
    pub(super) unit_name: String,
    pub(super) args: Vec<OsString>,
}

pub(super) fn network_helper_systemd_command(systemd_run_bin: &Path, helper_bin: &Path) -> Command {
    let invocation = build_network_helper_invocation(systemd_run_bin, helper_bin);
    let mut command = Command::new(invocation.program);
    command.env_clear().args(invocation.args);
    command
}

fn build_network_helper_invocation(
    systemd_run_bin: &Path,
    helper_bin: &Path,
) -> NetworkHelperSystemdInvocation {
    let sequence = NET_HELPER_UNIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    build_network_helper_invocation_with_sequence(
        systemd_run_bin,
        helper_bin,
        std::process::id(),
        sequence,
    )
}

fn build_network_helper_invocation_with_sequence(
    systemd_run_bin: &Path,
    helper_bin: &Path,
    process_id: u32,
    sequence: u64,
) -> NetworkHelperSystemdInvocation {
    let unit_name = net_helper_unit_name(helper_bin, process_id, sequence);
    let mut args = vec![
        OsString::from("--quiet"),
        OsString::from("--collect"),
        OsString::from("--wait"),
        OsString::from("--pipe"),
        OsString::from(format!("--unit={unit_name}")),
    ];
    push_net_helper_properties(&mut args);
    args.push(helper_bin.as_os_str().to_owned());
    NetworkHelperSystemdInvocation {
        program: systemd_run_bin.to_path_buf(),
        unit_name,
        args,
    }
}

fn push_net_helper_properties(args: &mut Vec<OsString>) {
    push_property(
        args,
        "CapabilityBoundingSet",
        NET_HELPER_CAPABILITY_BOUNDING_SET.join(" "),
    );
    push_property(
        args,
        "AmbientCapabilities",
        NET_HELPER_CAPABILITY_BOUNDING_SET.join(" "),
    );
    push_property(args, "NoNewPrivileges", "yes");
    push_property(args, "UMask", "0077");
    push_property(args, "SupplementaryGroups", "");
    push_property(args, "Environment", "");
    push_property(args, "KeyringMode", "private");
    push_property(args, "RestrictSUIDSGID", "yes");
    push_property(
        args,
        "RestrictAddressFamilies",
        "AF_UNIX AF_NETLINK AF_INET",
    );
    push_property(args, "RestrictNamespaces", "net");
    push_property(args, "LockPersonality", "yes");
    push_property(args, "SystemCallArchitectures", "native");
    push_property(
        args,
        "SystemCallFilter",
        "@system-service @network-io @mount",
    );
}

fn push_property(args: &mut Vec<OsString>, name: &str, value: impl AsRef<str>) {
    args.push(OsString::from(format!(
        "--property={name}={}",
        value.as_ref()
    )));
}

fn net_helper_unit_name(helper_bin: &Path, process_id: u32, sequence: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(helper_bin.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(process_id.to_ne_bytes());
    hasher.update(sequence.to_ne_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("{NET_HELPER_UNIT_PREFIX}-{}", &digest[..24])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(invocation: &NetworkHelperSystemdInvocation) -> Vec<String> {
        invocation
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn properties(invocation: &NetworkHelperSystemdInvocation) -> Vec<String> {
        strings(invocation)
            .into_iter()
            .filter_map(|arg| arg.strip_prefix("--property=").map(str::to_owned))
            .collect()
    }

    #[test]
    fn net_helper_directive_snapshot_is_pinned() {
        let invocation = build_network_helper_invocation_with_sequence(
            Path::new("/usr/bin/systemd-run"),
            Path::new("/opt/m80/bin/m80-net-helper"),
            1234,
            7,
        );

        assert_eq!(
            invocation.unit_name,
            "m80-net-helper-ada7fb38a89b98d87683839e"
        );
        assert_eq!(
            strings(&invocation).into_iter().take(5).collect::<Vec<_>>(),
            [
                "--quiet",
                "--collect",
                "--wait",
                "--pipe",
                "--unit=m80-net-helper-ada7fb38a89b98d87683839e",
            ]
        );
        assert_eq!(
            properties(&invocation),
            [
                "CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_ADMIN",
                "AmbientCapabilities=CAP_NET_ADMIN CAP_SYS_ADMIN",
                "NoNewPrivileges=yes",
                "UMask=0077",
                "SupplementaryGroups=",
                "Environment=",
                "KeyringMode=private",
                "RestrictSUIDSGID=yes",
                "RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_INET",
                "RestrictNamespaces=net",
                "LockPersonality=yes",
                "SystemCallArchitectures=native",
                "SystemCallFilter=@system-service @network-io @mount",
            ]
        );
        assert_eq!(
            strings(&invocation).last().map(String::as_str),
            Some("/opt/m80/bin/m80-net-helper")
        );
    }

    #[test]
    fn net_helper_unit_name_is_not_based_on_helper_basename() {
        let first = net_helper_unit_name(Path::new("/a/m80-net-helper"), 1, 1);
        let second = net_helper_unit_name(Path::new("/b/m80-net-helper"), 1, 1);
        let weird = net_helper_unit_name(Path::new("/tmp/bad name\n"), 1, 1);

        assert_ne!(first, second);
        assert!(first.starts_with("m80-net-helper-"));
        assert!(weird
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    }
}
