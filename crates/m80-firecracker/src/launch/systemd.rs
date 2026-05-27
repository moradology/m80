use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use m80_jailer::{CgroupVersion, JailerConfig, ResourceLimits};
use sha2::{Digest, Sha256};

const VM_UNIT_PREFIX: &str = "m80-vm";
const VM_LAUNCH_CAPABILITY_BOUNDING_SET: &[&str] = &[
    "CAP_CHOWN",
    "CAP_DAC_OVERRIDE",
    "CAP_SYS_CHROOT",
    "CAP_MKNOD",
    "CAP_SETUID",
    "CAP_SETGID",
    "CAP_SYS_ADMIN",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SystemdRunInvocation {
    pub(super) program: PathBuf,
    pub(super) args: Vec<OsString>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VmLaunchInvocationInput<'a> {
    pub(super) systemd_run_bin: &'a Path,
    pub(super) config: &'a JailerConfig,
    pub(super) api_socket_name: &'a OsStr,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(super) enum SystemdLaunchError {
    #[error("systemd launch does not support private cgroup namespace request")]
    CgroupNamespaceUnsupported,
}

pub(super) fn build_vm_launch_invocation(
    input: VmLaunchInvocationInput<'_>,
) -> Result<SystemdRunInvocation, SystemdLaunchError> {
    if input.config.new_cgroup_ns {
        return Err(SystemdLaunchError::CgroupNamespaceUnsupported);
    }

    let mut args = vec![
        OsString::from("--quiet"),
        OsString::from("--collect"),
        OsString::from(format!(
            "--unit={}",
            vm_launch_unit_name(&input.config.run_dir, input.config.run_dir.file_name())
        )),
    ];
    push_vm_launch_properties(&mut args, input.config);
    push_jailer_command(&mut args, input.config, input.api_socket_name);

    Ok(SystemdRunInvocation {
        program: input.systemd_run_bin.to_path_buf(),
        args,
    })
}

fn push_vm_launch_properties(args: &mut Vec<OsString>, config: &JailerConfig) {
    push_property(
        args,
        "CapabilityBoundingSet",
        VM_LAUNCH_CAPABILITY_BOUNDING_SET.join(" "),
    );
    push_property(args, "AmbientCapabilities", "");
    push_property(args, "NoNewPrivileges", "yes");
    push_property(args, "UMask", "0077");
    push_property(args, "SupplementaryGroups", "");
    push_property(args, "Environment", "");
    push_property(args, "KeyringMode", "private");
    push_property(args, "RestrictNamespaces", "~mnt pid net cgroup");
    push_property(args, "RestrictSUIDSGID", "yes");
    push_property(
        args,
        "RestrictAddressFamilies",
        "AF_UNIX AF_NETLINK AF_VSOCK",
    );
    push_property(args, "LockPersonality", "yes");
    push_property(args, "ProtectKernelModules", "yes");
    push_property(args, "ProtectKernelTunables", "yes");
    push_property(args, "ProtectKernelLogs", "yes");
    push_property(args, "ProtectClock", "yes");
    push_property(args, "PrivateDevices", "yes");
    push_property(args, "SystemCallArchitectures", "native");
    if config.new_net_ns {
        push_property(args, "PrivateNetwork", "yes");
    }
    match &config.stdio_log {
        Some(path) => {
            let target = format!("append:{}", path.display());
            push_property(args, "StandardOutput", target.as_str());
            push_property(args, "StandardError", target.as_str());
        }
        None => {
            push_property(args, "StandardOutput", "null");
            push_property(args, "StandardError", "null");
        }
    }
    push_resource_limits(args, &config.resource_limits);
}

fn push_resource_limits(args: &mut Vec<OsString>, limits: &ResourceLimits) {
    for (name, value) in [
        ("LimitNOFILE", Some(limits.no_file)),
        ("LimitFSIZE", limits.fsize),
        ("LimitNPROC", limits.nproc),
        ("LimitMEMLOCK", limits.memlock),
        ("LimitAS", limits.address_space),
        ("LimitCORE", limits.core),
        ("LimitSTACK", limits.stack),
    ] {
        if let Some(value) = value {
            push_property(args, name, value.to_string());
        }
    }
}

fn push_property(args: &mut Vec<OsString>, name: &str, value: impl AsRef<str>) {
    args.push(OsString::from(format!(
        "--property={name}={}",
        value.as_ref()
    )));
}

fn push_jailer_command(args: &mut Vec<OsString>, config: &JailerConfig, api_socket_name: &OsStr) {
    args.push(config.jailer_bin.as_os_str().to_owned());
    args.extend([
        OsString::from("--id"),
        run_dir_basename(&config.run_dir).to_owned(),
        OsString::from("--exec-file"),
        config.firecracker_bin.as_os_str().to_owned(),
        OsString::from("--uid"),
        OsString::from(config.uid.to_string()),
        OsString::from("--gid"),
        OsString::from(config.gid.to_string()),
        OsString::from("--chroot-base-dir"),
        config.run_dir.as_os_str().to_owned(),
        OsString::from("--resource-limit"),
        OsString::from(format!("no-file={}", config.resource_limits.no_file)),
    ]);

    if config.cgroup_version == Some(CgroupVersion::V2) {
        args.extend([OsString::from("--cgroup-version"), OsString::from("2")]);
    }
    if let Some(fsize) = config.resource_limits.fsize {
        args.extend([
            OsString::from("--resource-limit"),
            OsString::from(format!("fsize={fsize}")),
        ]);
    }
    if config.new_pid_ns {
        args.push(OsString::from("--new-pid-ns"));
    }
    if config.daemonize {
        args.push(OsString::from("--daemonize"));
    }
    if let Some(netns_path) = &config.netns_path {
        args.extend([OsString::from("--netns"), netns_path.as_os_str().to_owned()]);
    }

    args.extend([
        OsString::from("--"),
        OsString::from("--api-sock"),
        api_socket_name.to_owned(),
    ]);
    if let Some(seccomp_filter_path) = &config.seccomp_filter_path {
        args.extend([
            OsString::from("--seccomp-filter"),
            seccomp_filter_path.as_os_str().to_owned(),
        ]);
    }
}

fn vm_launch_unit_name(run_dir: &Path, vm_id: Option<&OsStr>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(run_dir.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    if let Some(vm_id) = vm_id {
        hasher.update(vm_id.as_encoded_bytes());
    }
    let digest = hex::encode(hasher.finalize());
    format!("{VM_UNIT_PREFIX}-{}", &digest[..24])
}

fn run_dir_basename(run_dir: &Path) -> OsString {
    run_dir
        .file_name()
        .expect("run_dir has a basename")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStrExt as _;

    use caps::Capability;

    use super::*;

    fn config() -> JailerConfig {
        JailerConfig {
            jailer_bin: "/opt/firecracker/bin/jailer".into(),
            jailer_harden_bin: Some("/opt/m80/bin/m80-jailer-harden".into()),
            firecracker_bin: "/opt/firecracker/bin/firecracker".into(),
            run_dir: "/run/m80/a/vm-1".into(),
            uid: 3000,
            gid: 3000,
            bindings: Vec::new(),
            sockets: Vec::new(),
            resource_limits: ResourceLimits {
                no_file: 4096,
                fsize: Some(8192),
                nproc: Some(64),
                memlock: Some(0),
                address_space: Some(1 << 30),
                core: Some(0),
                stack: Some(8 * 1024 * 1024),
            },
            new_pid_ns: true,
            new_net_ns: true,
            daemonize: false,
            new_cgroup_ns: false,
            cgroup_version: Some(CgroupVersion::V2),
            netns_path: None,
            seccomp_filter_path: Some("firecracker-seccomp-filter.bin".into()),
            stdio_log: Some("/run/m80/a/vm-1/console.log".into()),
        }
    }

    fn invocation(config: &JailerConfig) -> SystemdRunInvocation {
        build_vm_launch_invocation(VmLaunchInvocationInput {
            systemd_run_bin: Path::new("/usr/bin/systemd-run"),
            config,
            api_socket_name: OsStr::new("firecracker.sock"),
        })
        .unwrap()
    }

    fn strings(invocation: &SystemdRunInvocation) -> Vec<String> {
        invocation
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn properties(invocation: &SystemdRunInvocation) -> Vec<String> {
        strings(invocation)
            .into_iter()
            .filter_map(|arg| arg.strip_prefix("--property=").map(str::to_owned))
            .collect()
    }

    #[test]
    fn vm_launch_directive_snapshot_is_pinned() {
        let config = config();
        let invocation = invocation(&config);

        assert_eq!(
            properties(&invocation),
            [
                "CapabilityBoundingSet=CAP_CHOWN CAP_DAC_OVERRIDE CAP_SYS_CHROOT CAP_MKNOD CAP_SETUID CAP_SETGID CAP_SYS_ADMIN",
                "AmbientCapabilities=",
                "NoNewPrivileges=yes",
                "UMask=0077",
                "SupplementaryGroups=",
                "Environment=",
                "KeyringMode=private",
                "RestrictNamespaces=~mnt pid net cgroup",
                "RestrictSUIDSGID=yes",
                "RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_VSOCK",
                "LockPersonality=yes",
                "ProtectKernelModules=yes",
                "ProtectKernelTunables=yes",
                "ProtectKernelLogs=yes",
                "ProtectClock=yes",
                "PrivateDevices=yes",
                "SystemCallArchitectures=native",
                "PrivateNetwork=yes",
                "StandardOutput=append:/run/m80/a/vm-1/console.log",
                "StandardError=append:/run/m80/a/vm-1/console.log",
                "LimitNOFILE=4096",
                "LimitFSIZE=8192",
                "LimitNPROC=64",
                "LimitMEMLOCK=0",
                "LimitAS=1073741824",
                "LimitCORE=0",
                "LimitSTACK=8388608",
            ]
        );
    }

    #[test]
    fn capability_bounding_set_matches_wrapper_allowlist() {
        let wrapper_caps = m80_jailer_harden::OFFICIAL_JAILER_CAPABILITIES
            .iter()
            .copied()
            .map(capability_name)
            .collect::<Vec<_>>();

        assert_eq!(VM_LAUNCH_CAPABILITY_BOUNDING_SET, wrapper_caps);
    }

    #[test]
    fn jailer_command_argv_omits_wrapper_and_forwards_firecracker_args() {
        let config = config();
        let invocation = invocation(&config);
        let args = strings(&invocation);
        let jailer_index = args
            .iter()
            .position(|arg| arg == "/opt/firecracker/bin/jailer")
            .unwrap();

        assert!(!args.iter().any(|arg| arg.contains("m80-jailer-harden")));
        assert_eq!(
            &args[jailer_index..],
            [
                "/opt/firecracker/bin/jailer",
                "--id",
                "vm-1",
                "--exec-file",
                "/opt/firecracker/bin/firecracker",
                "--uid",
                "3000",
                "--gid",
                "3000",
                "--chroot-base-dir",
                "/run/m80/a/vm-1",
                "--resource-limit",
                "no-file=4096",
                "--cgroup-version",
                "2",
                "--resource-limit",
                "fsize=8192",
                "--new-pid-ns",
                "--",
                "--api-sock",
                "firecracker.sock",
                "--seccomp-filter",
                "firecracker-seccomp-filter.bin",
            ]
        );
    }

    #[test]
    fn missing_stdio_log_routes_unit_output_to_null() {
        let mut config = config();
        config.new_net_ns = false;
        config.stdio_log = None;
        let invocation = invocation(&config);
        let props = properties(&invocation);

        assert!(props.contains(&"StandardOutput=null".to_owned()));
        assert!(props.contains(&"StandardError=null".to_owned()));
        assert!(!props.contains(&"PrivateNetwork=yes".to_owned()));
    }

    #[test]
    fn unit_name_is_systemd_safe_and_not_based_only_on_basename() {
        let a = vm_launch_unit_name(Path::new("/run/m80/a/vm-1"), Some(OsStr::new("vm-1")));
        let b = vm_launch_unit_name(Path::new("/run/m80/b/vm-1"), Some(OsStr::new("vm-1")));
        let weird = vm_launch_unit_name(
            Path::new("/run/m80/a/../../bad name"),
            Some(OsStr::from_bytes(b"bad name\n")),
        );

        assert_ne!(a, b);
        assert!(a.starts_with("m80-vm-"));
        assert!(weird
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    }

    #[test]
    fn private_cgroup_namespace_request_fails_closed() {
        let mut config = config();
        config.new_cgroup_ns = true;

        assert_eq!(
            build_vm_launch_invocation(VmLaunchInvocationInput {
                systemd_run_bin: Path::new("/usr/bin/systemd-run"),
                config: &config,
                api_socket_name: OsStr::new("firecracker.sock"),
            })
            .unwrap_err(),
            SystemdLaunchError::CgroupNamespaceUnsupported
        );
    }

    fn capability_name(capability: Capability) -> &'static str {
        match capability {
            Capability::CAP_CHOWN => "CAP_CHOWN",
            Capability::CAP_DAC_OVERRIDE => "CAP_DAC_OVERRIDE",
            Capability::CAP_SYS_CHROOT => "CAP_SYS_CHROOT",
            Capability::CAP_MKNOD => "CAP_MKNOD",
            Capability::CAP_SETUID => "CAP_SETUID",
            Capability::CAP_SETGID => "CAP_SETGID",
            Capability::CAP_SYS_ADMIN => "CAP_SYS_ADMIN",
            other => panic!("unexpected official jailer capability {other:?}"),
        }
    }
}
