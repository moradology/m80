use crate::json;
use crate::release::VersionIdentity;

/// `m80 version` — print version strings.
///
/// Reports binary version, wire-protocol version, and (best-effort) the
/// Firecracker version pin from the manifest beside `M80_ROOTFS_IMAGE`.
/// If the manifest can't be read, the Firecracker pin renders as
/// `"unknown"` rather than failing the subcommand.
pub(super) fn cmd_version(json_mode: bool) -> anyhow::Result<i32> {
    let identity = VersionIdentity::current();
    let protocol_version = m80_proto::PROTOCOL_VERSION;
    let firecracker_pin = read_firecracker_pin();

    if json_mode {
        let obj = serde_json::json!({
            "binary_version": identity.binary_version,
            "package_version": identity.package_version,
            "release_tag": identity.release_tag,
            "release_build": identity.release_build,
            "version_status": identity.version_status,
            "expected_release_tag": identity.expected_release_tag,
            "protocol_version": protocol_version,
            "firecracker_pin": firecracker_pin,
        });
        println!("{}", json::to_pretty(&obj));
    } else {
        println!("m80             {}", identity.binary_version);
        println!("package         {}", identity.package_version);
        println!(
            "release         {}",
            identity.release_tag.as_deref().unwrap_or("unreleased")
        );
        println!("version_status  {}", identity.version_status.as_str());
        println!("protocol        {protocol_version}");
        println!("firecracker     {firecracker_pin}");
    }

    Ok(0)
}

/// Best-effort read of the Firecracker version pin from the manifest beside
/// `M80_ROOTFS_IMAGE`. Returns `"unknown"` when the env var is unset or the
/// manifest can't be parsed (this subcommand must never fail).
fn read_firecracker_pin() -> String {
    let Ok(rootfs) = std::env::var("M80_ROOTFS_IMAGE") else {
        return "unknown (set M80_ROOTFS_IMAGE)".to_string();
    };
    let manifest_path = format!("{rootfs}.manifest.json");
    match m80_image_manifest::Manifest::read(std::path::Path::new(&manifest_path)) {
        Ok(m) => m.expected_firecracker_version,
        Err(_) => "unknown".to_string(),
    }
}
