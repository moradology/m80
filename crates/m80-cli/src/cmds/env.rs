use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::EffectiveConfig;
use serde::Serialize;

use crate::config;
use crate::json;
use crate::profile::{self, ProfileBodySource, ProfileFilePaths};

mod render;

const DATA_VERSION: u16 = 1;

#[derive(Serialize)]
struct EnvDump {
    version: u16,
    cli_version: &'static str,
    protocol_version: u32,
    host: HostDump,
    config: ConfigDump,
    runtime_profile: RuntimeProfileDump,
    artifacts: ArtifactDump,
    firecracker: BinaryDump,
    run_root: RunRootDump,
    preflight: PreflightDump,
}

#[derive(Serialize)]
struct HostDump {
    kernel_version: Option<String>,
    kvm: DeviceCheck,
    vsock: ModuleCheck,
    cpu_count: Option<usize>,
    kvm_cpu_flags: Vec<String>,
    total_memory_kib: Option<u64>,
}

#[derive(Serialize)]
struct DeviceCheck {
    path: &'static str,
    exists: bool,
    read_write: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct ModuleCheck {
    loaded_or_available: bool,
    modules: Vec<String>,
}

#[derive(Serialize)]
struct ConfigDump {
    ok: bool,
    effective: Option<EffectiveConfig>,
    error: Option<String>,
}

#[derive(Serialize)]
struct RuntimeProfileDump {
    ok: bool,
    name: Option<String>,
    selection_source: Option<String>,
    body_source: Option<&'static str>,
    file_path: Option<PathBuf>,
    kernel_image: Option<PathBuf>,
    rootfs_image: Option<PathBuf>,
    kernel_kind: Option<String>,
    description: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct ArtifactDump {
    kernel_image: Option<PathBuf>,
    rootfs_image: Option<PathBuf>,
    kernel_kind: Option<String>,
    rootfs_manifest_path: Option<PathBuf>,
    rootfs_manifest_ok: bool,
    rootfs_manifest_error: Option<String>,
}

#[derive(Serialize)]
struct BinaryDump {
    path: PathBuf,
    exists: bool,
    version_output: Option<String>,
    error: Option<String>,
    configured_pin: Option<String>,
}

#[derive(Serialize)]
struct RunRootDump {
    path: Option<PathBuf>,
    exists: bool,
    run_dir_count: Option<usize>,
    error: Option<String>,
}

#[derive(Serialize)]
struct PreflightDump {
    ok: bool,
    error: Option<String>,
    checks: Vec<m80_preflight::CheckRow>,
}

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
    let runtime_profile = runtime_profile_dump(config_result.as_ref().ok());
    let artifacts = artifact_dump(&runtime_profile);
    let run_root = run_root_dump(config_result.as_ref().ok());
    let preflight = preflight_dump();

    EnvDump {
        version: DATA_VERSION,
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
        firecracker: firecracker_dump(),
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

fn runtime_profile_dump(effective: Option<&EffectiveConfig>) -> RuntimeProfileDump {
    let Some(effective) = effective else {
        return RuntimeProfileDump::error("effective config unavailable");
    };
    match profile::resolve_from_effective(effective, ProfileFilePaths::host()) {
        Ok(profile) => RuntimeProfileDump {
            ok: true,
            name: Some(profile.name),
            selection_source: Some(format!("{:?}", profile.selection_source)),
            body_source: Some(profile_body_source(profile.body_source)),
            file_path: profile.file_path,
            kernel_image: profile.kernel_image,
            rootfs_image: profile.rootfs_image,
            kernel_kind: profile.kernel_kind,
            description: profile.description,
            error: None,
        },
        Err(e) => RuntimeProfileDump::error(format!("{e}")),
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
            kernel_image: None,
            rootfs_image: None,
            kernel_kind: None,
            description: None,
            error: Some(error.into()),
        }
    }
}

fn profile_body_source(source: ProfileBodySource) -> &'static str {
    match source {
        ProfileBodySource::BuiltinEnv => "builtin_env",
        ProfileBodySource::SystemFile => "system_file",
        ProfileBodySource::UserFile => "user_file",
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

fn firecracker_dump() -> BinaryDump {
    let path = std::env::var_os(m80_preflight::ENV_FIRECRACKER_BIN)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(m80_preflight::DEFAULT_FIRECRACKER_BIN));
    let exists = path.exists();
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

fn preflight_dump() -> PreflightDump {
    match m80_preflight::run() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_json_has_data_version_and_host_sections() {
        let rendered = json::to_pretty(&collect_env_dump());
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["data"]["version"], 1);
        assert!(parsed["data"]["host"]["kvm"].is_object());
        assert!(parsed["data"]["config"].is_object());
        assert!(parsed["data"]["firecracker"].is_object());
    }

    #[test]
    fn human_output_names_bug_report_fields() {
        let text = render::render_env_human(&collect_env_dump());

        assert!(text.contains("kvm:"));
        assert!(text.contains("firecracker:"));
        assert!(text.contains("run_root:"));
        assert!(text.contains("preflight:"));
    }
}
