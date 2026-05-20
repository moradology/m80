use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_firecracker::EffectiveConfig;
use serde::Serialize;

use crate::config;
use crate::json;
use crate::profile::{self, ProfileFilePaths, RuntimeProfile};

mod render;

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
    artifact_dir: Option<PathBuf>,
    kernel_image: Option<PathBuf>,
    rootfs_image: Option<PathBuf>,
    kernel_kind: Option<String>,
    guestd: Option<PathBuf>,
    guest_manifest: Option<PathBuf>,
    build_receipt: Option<PathBuf>,
    install_provenance: Option<PathBuf>,
    host_binaries_manifest: Option<PathBuf>,
    firecracker_bin: Option<PathBuf>,
    firecracker_seccomp_filter: Option<PathBuf>,
    jailer_bin: Option<PathBuf>,
    jailer_harden_bin: Option<PathBuf>,
    net_helper_bin: Option<PathBuf>,
    run_root: Option<PathBuf>,
    release_tag: Option<String>,
    m80_version: Option<String>,
    description: Option<String>,
    active_pointer: Option<PathBuf>,
    active_pointer_target: Option<PathBuf>,
    active_pointer_status: Option<&'static str>,
    active_pointer_error: Option<String>,
    missing_paths: Vec<profile::RuntimeProfilePathIssue>,
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
    seccomp_filter_path: PathBuf,
    seccomp_filter_exists: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn env_json_has_data_version_and_host_sections() {
        let rendered = json::to_pretty(&collect_env_dump());
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["data"]["version"], 1);
        assert!(parsed["data"]["host"]["kvm"].is_object());
        assert!(parsed["data"]["config"].is_object());
        assert!(parsed["data"]["firecracker"].is_object());
        assert!(parsed["data"]["firecracker"]["seccomp_filter_path"].is_string());
    }

    #[test]
    fn human_output_names_bug_report_fields() {
        let text = render::render_env_human(&collect_env_dump());

        assert!(text.contains("kvm:"));
        assert!(text.contains("firecracker:"));
        assert!(text.contains("firecracker_seccomp_filter:"));
        assert!(text.contains("run_root:"));
        assert!(text.contains("preflight:"));
    }

    #[test]
    fn env_json_reports_selected_installed_profile_paths() {
        let _lock = m80_test_helpers::env::env_lock().lock().unwrap();
        let _restore = m80_test_helpers::env::EnvRestore::capture(&[
            "HOME",
            "M80_DEFAULT_PROFILE",
            "M80_RUN_ROOT",
            "M80_ARTIFACT_DIR",
            "M80_KERNEL_IMAGE",
            "M80_ROOTFS_IMAGE",
            "M80_KERNEL_KIND",
            m80_preflight::ENV_FIRECRACKER_BIN,
            m80_preflight::ENV_FIRECRACKER_SECCOMP_FILTER,
            m80_preflight::ENV_FIRECRACKER_VERSION,
        ]);
        for key in [
            "M80_ARTIFACT_DIR",
            "M80_KERNEL_IMAGE",
            "M80_ROOTFS_IMAGE",
            "M80_KERNEL_KIND",
            m80_preflight::ENV_FIRECRACKER_BIN,
            m80_preflight::ENV_FIRECRACKER_SECCOMP_FILTER,
            m80_preflight::ENV_FIRECRACKER_VERSION,
        ] {
            std::env::remove_var(key);
        }

        let home = tempfile::tempdir().unwrap();
        let profile_dir = home.path().join(".config/m80/profiles");
        let install_root = home.path().join("install");
        let artifacts = install_root.join("versions/v1/artifacts");
        let bin = install_root.join("versions/v1/bin");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let firecracker = bin.join("firecracker");
        std::fs::write(&firecracker, "#!/bin/sh\nprintf 'Firecracker v1.15.1\\n'\n").unwrap();
        let mut mode = std::fs::metadata(&firecracker).unwrap().permissions();
        mode.set_mode(0o755);
        std::fs::set_permissions(&firecracker, mode).unwrap();
        let seccomp = bin.join("firecracker-seccomp-filter.bin");
        std::fs::write(&seccomp, "{}\n").unwrap();
        let run_root = home.path().join("run-root");

        std::fs::write(
            profile_dir.join("default.toml"),
            format!(
                "artifact_dir = '{}'\n\
                 kernel_image = '{}'\n\
                 rootfs_image = '{}'\n\
                 kernel_kind = 'stripped'\n\
                 firecracker_bin = '{}'\n\
                 firecracker_seccomp_filter = '{}'\n\
                 jailer_bin = '{}'\n\
                 jailer_harden_bin = '{}'\n\
                 net_helper_bin = '{}'\n\
                 run_root = '{}'\n\
                 release_tag = 'v1'\n\
                 m80_version = 'v1'\n",
                artifacts.display(),
                artifacts.join("vmlinux").display(),
                artifacts.join("output.ext4").display(),
                firecracker.display(),
                seccomp.display(),
                bin.join("jailer").display(),
                bin.join("m80-jailer-harden").display(),
                bin.join("m80-net-helper").display(),
                run_root.display()
            ),
        )
        .unwrap();
        std::env::set_var("HOME", home.path());
        std::env::set_var("M80_DEFAULT_PROFILE", "default");
        std::env::set_var("M80_RUN_ROOT", &run_root);

        let rendered = json::to_pretty(&collect_env_dump());
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let profile = &parsed["data"]["runtime_profile"];

        assert_eq!(profile["name"], "default");
        assert_eq!(profile["selection_source"], "Env");
        assert_eq!(profile["body_source"], "user_file");
        assert_eq!(
            profile["file_path"].as_str(),
            profile_dir.join("default.toml").to_str()
        );
        assert_eq!(profile["artifact_dir"].as_str(), artifacts.to_str());
        assert_eq!(
            profile["kernel_image"].as_str(),
            artifacts.join("vmlinux").to_str()
        );
        assert_eq!(
            profile["rootfs_image"].as_str(),
            artifacts.join("output.ext4").to_str()
        );
        assert_eq!(profile["firecracker_bin"].as_str(), firecracker.to_str());
        assert_eq!(
            profile["firecracker_seccomp_filter"].as_str(),
            seccomp.to_str()
        );
        assert_eq!(profile["run_root"].as_str(), run_root.to_str());
        assert_eq!(
            profile["active_pointer"].as_str(),
            install_root.join("active").to_str()
        );
        assert_eq!(profile["active_pointer_status"], "missing");
        let missing_paths = profile["missing_paths"].as_array().unwrap();
        assert!(
            missing_paths
                .iter()
                .any(|missing| missing["field"] == "rootfs_image"
                    && missing["path"] == artifacts.join("output.ext4").to_str().unwrap()
                    && missing["reason"] == "missing"),
            "{missing_paths:?}"
        );
        assert_eq!(
            parsed["data"]["artifacts"]["kernel_image"].as_str(),
            artifacts.join("vmlinux").to_str()
        );
        assert_eq!(
            parsed["data"]["artifacts"]["rootfs_image"].as_str(),
            artifacts.join("output.ext4").to_str()
        );
        assert_eq!(
            parsed["data"]["firecracker"]["path"].as_str(),
            firecracker.to_str()
        );
        assert_eq!(
            parsed["data"]["run_root"]["path"].as_str(),
            run_root.to_str()
        );
    }
}
