use std::path::PathBuf;

const FIRECRACKER_POLICY_TAG: &str = "v1.15.1";

const REQUIRED_BUILTINS: &[&str] = &[
    "CONFIG_VIRTIO",
    "CONFIG_VIRTIO_MMIO",
    "CONFIG_VIRTIO_MMIO_CMDLINE_DEVICES",
    "CONFIG_VIRTIO_BLK",
    "CONFIG_BLK_MQ_VIRTIO",
    "CONFIG_VIRTIO_NET",
    "CONFIG_VIRTIO_VSOCKETS",
    "CONFIG_VSOCKETS",
    "CONFIG_HW_RANDOM_VIRTIO",
    "CONFIG_RANDOM_TRUST_CPU",
    "CONFIG_HYPERVISOR_GUEST",
    "CONFIG_KVM_GUEST",
    "CONFIG_PVH",
    "CONFIG_SERIAL_8250",
    "CONFIG_SERIAL_8250_CONSOLE",
    "CONFIG_PRINTK",
];

fn kernel_builder_config() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set (run via cargo test)");
    PathBuf::from(manifest_dir)
        .join("kernel-builder")
        .join("m80-stripped.config")
}

fn committed_config_text() -> String {
    std::fs::read_to_string(kernel_builder_config()).expect("m80-stripped.config must be readable")
}

fn config_symbol_value<'a>(config: &'a str, symbol: &str) -> Option<&'a str> {
    let enabled_prefix = format!("{symbol}=");
    let disabled_line = format!("# {symbol} is not set");

    for line in config.lines() {
        if let Some(value) = line.strip_prefix(&enabled_prefix) {
            return Some(value);
        }
        if line == disabled_line {
            return Some("not set");
        }
    }

    None
}

#[test]
fn firecracker_policy_required_legacy_mmio_symbols_are_builtin() {
    let config = committed_config_text();

    for symbol in REQUIRED_BUILTINS {
        assert_eq!(
            config_symbol_value(&config, symbol),
            Some("y"),
            "{symbol} must be built in for Firecracker {FIRECRACKER_POLICY_TAG} x86_64 legacy-MMIO boot; see docs/behaviors/kernel/config-completeness.md"
        );
    }
}
