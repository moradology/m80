use std::path::Path;

use super::EnvDump;

pub(super) fn render_env_human(dump: &EnvDump) -> String {
    let mut out = String::new();
    out.push_str(&format!("m80: {}\n", dump.cli_version));
    out.push_str(&format!("protocol: {}\n", dump.protocol_version));
    out.push_str(&format!(
        "kernel: {}\n",
        dump.host.kernel_version.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!(
        "cpu_count: {}\n",
        dump.host
            .cpu_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    ));
    out.push_str(&format!(
        "kvm: {} ({})\n",
        if dump.host.kvm.read_write {
            "read-write"
        } else if dump.host.kvm.exists {
            "present"
        } else {
            "missing"
        },
        dump.host.kvm.error.as_deref().unwrap_or("open succeeded")
    ));
    out.push_str(&format!(
        "vsock: {} {:?}\n",
        if dump.host.vsock.loaded_or_available {
            "present"
        } else {
            "missing"
        },
        dump.host.vsock.modules
    ));
    out.push_str(&format!(
        "config: {}\n",
        if dump.config.ok { "ok" } else { "error" }
    ));
    if let Some(error) = &dump.config.error {
        out.push_str(&format!("  error: {error}\n"));
    }
    out.push_str(&format!(
        "profile: {}\n",
        dump.runtime_profile.name.as_deref().unwrap_or("unresolved")
    ));
    if let Some(error) = &dump.runtime_profile.error {
        out.push_str(&format!("  error: {error}\n"));
    }
    out.push_str(&format!(
        "kernel_image: {}\n",
        display_optional_path(dump.artifacts.kernel_image.as_deref())
    ));
    out.push_str(&format!(
        "rootfs_image: {}\n",
        display_optional_path(dump.artifacts.rootfs_image.as_deref())
    ));
    out.push_str(&format!(
        "rootfs_manifest: {}\n",
        if dump.artifacts.rootfs_manifest_ok {
            "ok".to_owned()
        } else {
            dump.artifacts
                .rootfs_manifest_error
                .clone()
                .unwrap_or_else(|| "unavailable".to_owned())
        }
    ));
    out.push_str(&format!(
        "firecracker: {} ({})\n",
        dump.firecracker.path.display(),
        dump.firecracker
            .version_output
            .as_deref()
            .or(dump.firecracker.error.as_deref())
            .unwrap_or("unknown")
    ));
    out.push_str(&format!(
        "run_root: {} exists={} run_dirs={}\n",
        display_optional_path(dump.run_root.path.as_deref()),
        dump.run_root.exists,
        dump.run_root
            .run_dir_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    ));
    out.push_str(&format!(
        "preflight: {}\n",
        if dump.preflight.ok { "ok" } else { "error" }
    ));
    if let Some(error) = &dump.preflight.error {
        out.push_str(&format!("  error: {error}\n"));
    }
    out
}

fn display_optional_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_owned())
}
