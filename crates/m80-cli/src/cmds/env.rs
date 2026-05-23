use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::EffectiveConfig;

use crate::config;
use crate::json;
use crate::profile::{self, ProfileFilePaths, RuntimeProfile};

mod model;
mod render;
#[cfg(test)]
mod tests;

use model::{
    ArtifactDump, BinaryDump, ConfigDump, DeviceCheck, EnvDump, HostDump, ModuleCheck,
    PreflightDump, RunRootDump, RuntimeProfileDump,
};

pub(super) fn cmd_env(json: bool) -> anyhow::Result<i32> {
    let dump = collect_env_dump();
    if json {
        println!("{}", json::to_pretty(&dump));
    } else {
        print!("{}", render::render_env_human(&dump));
    }
    Ok(0)
}

fn collect_env_dump() -> EnvDump {
    let config_result = config::load_effective(&HashMap::new());
    let runtime_profile_result = match config_result.as_ref() {
        Ok(effective) => profile::resolve_from_effective(effective, ProfileFilePaths::host())
            .map_err(|e| e.to_string()),
        Err(_) => Err("effective config unavailable".to_owned()),
    };
    let runtime_profile = runtime_profile_dump(runtime_profile_result.as_ref());
    let artifacts = artifact_dump(&runtime_profile);
    let run_root = run_root_dump(config_result.as_ref().ok());
    let preflight = preflight_dump(config_result.as_ref().ok());
    let firecracker = firecracker_dump(&runtime_profile);

    EnvDump {
        version: 1,
        cli_version: env!("CARGO_PKG_VERSION"),
        protocol_version: m80_proto::PROTOCOL_VERSION,
        host: host_dump(),
        config: match config_result {
            Ok(effective) => ConfigDump {
                ok: true,
                effective: Some(effective),
                error: None,
            },
            Err(e) => ConfigDump {
                ok: false,
                effective: None,
                error: Some(format!("{e:#}")),
            },
        },
        runtime_profile,
        artifacts,
        firecracker,
        run_root,
        preflight,
    }
}

fn host_dump() -> HostDump {
    HostDump {
        kernel_version: read_trimmed("/proc/sys/kernel/osrelease"),
        kvm: check_kvm(),
        vsock: check_vsock(),
        cpu_count: std::thread::available_parallelism().ok().map(usize::from),
        kvm_cpu_flags: kvm_cpu_flags(),
        total_memory_kib: mem_total_kib(),
    }
}

fn check_kvm() -> DeviceCheck {
    let path = Path::new("/dev/kvm");
    let exists = path.exists();
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(_) => DeviceCheck {
            path: "/dev/kvm",
            exists,
            read_write: true,
            error: None,
        },
        Err(e) => DeviceCheck {
            path: "/dev/kvm",
            exists,
            read_write: false,
            error: Some(e.to_string()),
        },
    }
}

fn check_vsock() -> ModuleCheck {
    let mut modules = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/proc/modules") {
        for name in ["vsock", "vhost_vsock", "vmw_vsock_virtio_transport_common"] {
            if text.lines().any(|line| line.starts_with(name)) {
                modules.push(name.to_owned());
            }
        }
    }
    for name in ["vsock", "vhost_vsock"] {
        let module_path = PathBuf::from("/sys/module").join(name);
        if module_path.exists() && !modules.iter().any(|module| module == name) {
            modules.push(name.to_owned());
        }
    }
    ModuleCheck {
        loaded_or_available: !modules.is_empty(),
        modules,
    }
}

fn kvm_cpu_flags() -> Vec<String> {
    let Some(text) = read_trimmed("/proc/cpuinfo") else {
        return Vec::new();
    };
    let mut flags = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("flags") else {
            continue;
        };
        let Some((_, values)) = rest.split_once(':') else {
            continue;
        };
        for flag in values.split_whitespace() {
            if matches!(flag, "vmx" | "svm") && !flags.iter().any(|seen| seen == flag) {
                flags.push(flag.to_owned());
            }
        }
    }
    flags
}

fn mem_total_kib() -> Option<u64> {
    let text = read_trimmed("/proc/meminfo")?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn runtime_profile_dump(result: Result<&RuntimeProfile, &String>) -> RuntimeProfileDump {
    match result {
        Ok(profile) => {
            let report = profile::runtime_profile_report(profile);
            RuntimeProfileDump {
                ok: true,
                name: Some(report.name),
                selection_source: Some(report.selection_source),
                body_source: Some(report.body_source),
                file_path: report.file_path,
                artifact_dir: report.artifact_dir,
                kernel_image: report.kernel_image,
                rootfs_image: report.rootfs_image,
                kernel_kind: report.kernel_kind,
                guestd: report.guestd,
                guest_manifest: report.guest_manifest,
                build_receipt: report.build_receipt,
                install_provenance: report.install_provenance,
                host_binaries_manifest: report.host_binaries_manifest,
                firecracker_bin: report.firecracker_bin,
                firecracker_seccomp_filter: report.firecracker_seccomp_filter,
                jailer_bin: report.jailer_bin,
                jailer_harden_bin: report.jailer_harden_bin,
                net_helper_bin: report.net_helper_bin,
                run_root: report.run_root,
                release_tag: report.release_tag,
                m80_version: report.m80_version,
                description: report.description,
                active_pointer: report.active_pointer,
                active_pointer_target: report.active_pointer_target,
                active_pointer_status: report.active_pointer_status,
                active_pointer_error: report.active_pointer_error,
                missing_paths: report.missing_paths,
                error: None,
            }
        }
        Err(e) => RuntimeProfileDump::error(e.clone()),
    }
}

impl RuntimeProfileDump {
    fn error(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            name: None,
            selection_source: None,
            body_source: None,
            file_path: None,
            artifact_dir: None,
            kernel_image: None,
            rootfs_image: None,
            kernel_kind: None,
            guestd: None,
            guest_manifest: None,
            build_receipt: None,
            install_provenance: None,
            host_binaries_manifest: None,
            firecracker_bin: None,
            firecracker_seccomp_filter: None,
            jailer_bin: None,
            jailer_harden_bin: None,
            net_helper_bin: None,
            run_root: None,
            release_tag: None,
            m80_version: None,
            description: None,
            active_pointer: None,
            active_pointer_target: None,
            active_pointer_status: None,
            active_pointer_error: None,
            missing_paths: Vec::new(),
            error: Some(error.into()),
        }
    }
}

fn artifact_dump(profile: &RuntimeProfileDump) -> ArtifactDump {
    let kernel_image = profile
        .kernel_image
        .clone()
        .or_else(|| std::env::var_os("M80_KERNEL_IMAGE").map(PathBuf::from));
    let rootfs_image = profile
        .rootfs_image
        .clone()
        .or_else(|| std::env::var_os("M80_ROOTFS_IMAGE").map(PathBuf::from));
    let kernel_kind = profile
        .kernel_kind
        .clone()
        .or_else(|| std::env::var("M80_KERNEL_KIND").ok());
    let rootfs_manifest_path = rootfs_image.as_ref().map(|path| {
        let text = path.display().to_string();
        PathBuf::from(format!("{text}.manifest.json"))
    });
    let (rootfs_manifest_ok, rootfs_manifest_error) = match rootfs_manifest_path
        .as_ref()
        .map(|path| m80_image_manifest::Manifest::read(path).map(|_| ()))
    {
        Some(Ok(())) => (true, None),
        Some(Err(e)) => (false, Some(e.to_string())),
        None => (false, Some("rootfs image path unavailable".to_owned())),
    };

    ArtifactDump {
        kernel_image,
        rootfs_image,
        kernel_kind,
        rootfs_manifest_path,
        rootfs_manifest_ok,
        rootfs_manifest_error,
    }
}

fn firecracker_dump(profile: &RuntimeProfileDump) -> BinaryDump {
    let path = profile
        .firecracker_bin
        .clone()
        .or_else(|| std::env::var_os(m80_preflight::ENV_FIRECRACKER_BIN).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(m80_preflight::DEFAULT_FIRECRACKER_BIN));
    let exists = path.exists();
    let seccomp_filter_path = profile
        .firecracker_seccomp_filter
        .clone()
        .or_else(|| {
            std::env::var_os(m80_preflight::ENV_FIRECRACKER_SECCOMP_FILTER).map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from(m80_preflight::DEFAULT_FIRECRACKER_SECCOMP_FILTER));
    let seccomp_filter_exists = seccomp_filter_path.exists();
    let configured_pin = std::env::var(m80_preflight::ENV_FIRECRACKER_VERSION).ok();
    let mut error = None;
    let version_output = match Command::new(&path).arg("--version").output() {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_owned()
            } else {
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            };
            if output.status.success() {
                Some(text)
            } else {
                error = Some(format!("firecracker --version exited {}", output.status));
                (!text.is_empty()).then_some(text)
            }
        }
        Err(e) => {
            error = Some(e.to_string());
            None
        }
    };

    BinaryDump {
        path,
        exists,
        seccomp_filter_path,
        seccomp_filter_exists,
        version_output,
        error,
        configured_pin,
    }
}

fn run_root_dump(effective: Option<&EffectiveConfig>) -> RunRootDump {
    let Some(effective) = effective else {
        return RunRootDump {
            path: None,
            exists: false,
            run_dir_count: None,
            error: Some("effective config unavailable".to_owned()),
        };
    };
    let path = effective
        .fields
        .iter()
        .find(|field| field.name == "run_root")
        .map(|field| PathBuf::from(&field.value));
    let Some(path) = path else {
        return RunRootDump {
            path: None,
            exists: false,
            run_dir_count: None,
            error: Some("run_root field missing".to_owned()),
        };
    };
    let exists = path.exists();
    let (run_dir_count, error) = if exists {
        match std::fs::read_dir(&path) {
            Ok(entries) => (
                Some(
                    entries
                        .flatten()
                        .filter(|entry| entry.path().is_dir())
                        .count(),
                ),
                None,
            ),
            Err(e) => (None, Some(e.to_string())),
        }
    } else {
        (Some(0), None)
    };
    RunRootDump {
        path: Some(path),
        exists,
        run_dir_count,
        error,
    }
}

fn preflight_dump(effective: Option<&EffectiveConfig>) -> PreflightDump {
    let Some(effective) = effective else {
        return PreflightDump {
            ok: false,
            error: Some("effective config unavailable".to_owned()),
            checks: Vec::new(),
        };
    };
    match super::preflight::preflight_with_effective_config(effective.clone()) {
        Ok(discovery) => PreflightDump {
            ok: true,
            error: None,
            checks: discovery.report,
        },
        Err(e) => PreflightDump {
            ok: false,
            error: Some(e.to_string()),
            checks: Vec::new(),
        },
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}
