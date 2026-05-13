use std::fs;
use std::path::Path;

use m80_firecracker::{SandboxConfig, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT};

#[test]
fn v0_2_first_line_shape() {
    assert_eq!(FIRST_LINE_VCPU_COUNT, 1);
    assert_eq!(FIRST_LINE_MEM_SIZE_MIB, 512);

    let config = SandboxConfig::default();
    assert!(
        config.vcpu_count.is_none(),
        "omitted vCPU sizing selects the first-line default at preboot"
    );
    assert!(
        config.mem_size_mib.is_none(),
        "omitted memory sizing selects the first-line default at preboot"
    );
}

#[test]
fn snapshot_timing_fixtures_use_first_line_shape() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "benches/snapshot_restore_latency.rs",
        "benches/warm_pool_allocation_latency.rs",
    ] {
        let path = crate_root.join(relative);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));

        assert!(
            source.contains("FIRST_LINE_VCPU_COUNT"),
            "{relative} must pin vCPU count through the first-line contract"
        );
        assert!(
            source.contains("FIRST_LINE_MEM_SIZE_MIB"),
            "{relative} must pin memory through the first-line contract"
        );
        assert!(
            !source.contains("mem_size_mib: Some(512)"),
            "{relative} must use the first-line memory constant, not a hard-coded 512 MiB shape"
        );
    }
}
