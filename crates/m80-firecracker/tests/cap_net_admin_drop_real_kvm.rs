//! Real-host proof that backend init drops CAP_NET_ADMIN from the backend thread.

use m80_firecracker::{Backend, BackendConfig, CgroupMode};

const CAP_NET_ADMIN_MASK: u64 = 1u64 << 12;

#[test]
#[ignore = "requires real preflight artifacts and irreversibly drops CAP_NET_ADMIN in this test process"]
fn backend_init_drops_parent_cap_net_admin() {
    let discovery = m80_preflight::run().expect("preflight");
    let backend = Backend::new(
        BackendConfig::builder(discovery)
            .cgroup_mode(CgroupMode::Disabled)
            .build(),
    )
    .expect("Backend::new");
    drop(backend);

    let status = std::fs::read_to_string("/proc/thread-self/status").expect("status");
    for field in ["CapEff", "CapPrm", "CapBnd"] {
        let value = status_hex_value(&status, field);
        assert_eq!(
            value & CAP_NET_ADMIN_MASK,
            0,
            "{field} still contains CAP_NET_ADMIN"
        );
    }
}

fn status_hex_value(status: &str, field: &str) -> u64 {
    let prefix = format!("{field}:\t");
    let raw = status
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("{field} missing from /proc/thread-self/status"));
    u64::from_str_radix(raw.trim(), 16).expect("hex capability field")
}
