use std::fmt::Write as _;
use std::path::Path;

use super::EnvDump;

pub(super) fn render_env_human(dump: &EnvDump) -> String {
    let mut out = String::new();
    writeln!(out, "m80: {}", dump.cli_version).unwrap();
    writeln!(out, "protocol: {}", dump.protocol_version).unwrap();
    writeln!(
        out,
        "kernel: {}",
        dump.host.kernel_version.as_deref().unwrap_or("unknown")
    )
    .unwrap();
    writeln!(
        out,
        "cpu_count: {}",
        dump.host
            .cpu_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    )
    .unwrap();
    writeln!(
        out,
        "kvm: {} ({})",
        if dump.host.kvm.read_write {
            "read-write"
        } else if dump.host.kvm.exists {
            "present"
        } else {
            "missing"
        },
        dump.host.kvm.error.as_deref().unwrap_or("open succeeded")
    )
    .unwrap();
    writeln!(
        out,
        "vsock: {} {:?}",
        if dump.host.vsock.loaded_or_available {
            "present"
        } else {
            "missing"
        },
        dump.host.vsock.modules
    )
    .unwrap();
    writeln!(
        out,
        "config: {}",
        if dump.config.ok { "ok" } else { "error" }
    )
    .unwrap();
    if let Some(error) = &dump.config.error {
        writeln!(out, "  error: {error}").unwrap();
    }
    writeln!(
        out,
        "profile: {}",
        dump.runtime_profile.name.as_deref().unwrap_or("unresolved")
    )
    .unwrap();
    if let Some(error) = &dump.runtime_profile.error {
        writeln!(out, "  error: {error}").unwrap();
    }
    writeln!(
        out,
        "kernel_image: {}",
        display_optional_path(dump.artifacts.kernel_image.as_deref())
    )
    .unwrap();
    writeln!(
        out,
        "rootfs_image: {}",
        display_optional_path(dump.artifacts.rootfs_image.as_deref())
    )
    .unwrap();
    writeln!(
        out,
        "rootfs_manifest: {}",
        if dump.artifacts.rootfs_manifest_ok {
            "ok".to_owned()
        } else {
            dump.artifacts
                .rootfs_manifest_error
                .clone()
                .unwrap_or_else(|| "unavailable".to_owned())
        }
    )
    .unwrap();
    writeln!(
        out,
        "firecracker: {} ({})",
        dump.firecracker.path.display(),
        dump.firecracker
            .version_output
            .as_deref()
            .or(dump.firecracker.error.as_deref())
            .unwrap_or("unknown")
    )
    .unwrap();
    writeln!(
        out,
        "firecracker_seccomp_filter: {} exists={}",
        dump.firecracker.seccomp_filter_path.display(),
        dump.firecracker.seccomp_filter_exists
    )
    .unwrap();
    writeln!(
        out,
        "run_root: {} exists={} run_dirs={}",
        display_optional_path(dump.run_root.path.as_deref()),
        dump.run_root.exists,
        dump.run_root
            .run_dir_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_owned())
    )
    .unwrap();
    writeln!(
        out,
        "preflight: {}",
        if dump.preflight.ok { "ok" } else { "error" }
    )
    .unwrap();
    if let Some(error) = &dump.preflight.error {
        writeln!(out, "  error: {error}").unwrap();
    }
    out
}

fn display_optional_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_owned())
}
