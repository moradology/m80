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
    if let Some(selection_source) = &dump.runtime_profile.selection_source {
        writeln!(out, "  selection_source: {selection_source}").unwrap();
    }
    if let Some(body_source) = dump.runtime_profile.body_source {
        writeln!(out, "  body_source: {body_source}").unwrap();
    }
    if let Some(file_path) = &dump.runtime_profile.file_path {
        writeln!(out, "  file_path: {}", file_path.display()).unwrap();
    }
    if let Some(run_root) = &dump.runtime_profile.run_root {
        writeln!(out, "  profile_run_root: {}", run_root.display()).unwrap();
    }
    if let Some(active_pointer) = &dump.runtime_profile.active_pointer {
        writeln!(
            out,
            "  active_pointer: {} status={}",
            active_pointer.display(),
            dump.runtime_profile
                .active_pointer_status
                .unwrap_or("unknown")
        )
        .unwrap();
    }
    if let Some(active_target) = &dump.runtime_profile.active_pointer_target {
        writeln!(out, "  active_pointer_target: {}", active_target.display()).unwrap();
    }
    if let Some(error) = &dump.runtime_profile.active_pointer_error {
        writeln!(out, "  active_pointer_error: {error}").unwrap();
    }
    if !dump.runtime_profile.missing_paths.is_empty() {
        writeln!(out, "  missing_paths:").unwrap();
        for missing in &dump.runtime_profile.missing_paths {
            writeln!(
                out,
                "    {}: {} ({})",
                missing.field,
                missing.path.display(),
                missing.reason
            )
            .unwrap();
        }
    }
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
    if let Some(jailer) = &dump.runtime_profile.jailer_bin {
        writeln!(out, "jailer: {}", jailer.display()).unwrap();
    }
    if let Some(jailer_harden) = &dump.runtime_profile.jailer_harden_bin {
        writeln!(out, "jailer_harden: {}", jailer_harden.display()).unwrap();
    }
    if let Some(net_helper) = &dump.runtime_profile.net_helper_bin {
        writeln!(out, "net_helper: {}", net_helper.display()).unwrap();
    }
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
