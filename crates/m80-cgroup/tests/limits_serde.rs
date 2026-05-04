//! JSON round-trip tests for `Limits` and `CpuMax`.

use m80_cgroup::{CpuMax, Limits};

#[test]
fn limits_default_round_trips() {
    let json = serde_json::to_string(&Limits::default()).expect("serialize");
    let _back: Limits = serde_json::from_str(&json).expect("deserialize");
}

#[test]
fn limits_with_all_fields_round_trips() {
    let orig = Limits {
        cpu_max: Some(CpuMax::Quota {
            quota_us: 100_000,
            period_us: 200_000,
        }),
        memory_max: Some(1_610_612_736),
        pids_max: Some(128),
    };
    let json = serde_json::to_string(&orig).expect("serialize");
    let back: Limits = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.memory_max, Some(1_610_612_736));
    assert_eq!(back.pids_max, Some(128));
    match back.cpu_max.expect("cpu_max present") {
        CpuMax::Quota { quota_us, period_us } => {
            assert_eq!(quota_us, 100_000);
            assert_eq!(period_us, 200_000);
        }
        CpuMax::Max => panic!("expected Quota variant"),
    }
}

#[test]
fn limits_with_cpu_max_variant_round_trips() {
    let orig = Limits {
        cpu_max: Some(CpuMax::Max),
        memory_max: None,
        pids_max: None,
    };
    let json = serde_json::to_string(&orig).expect("serialize");
    let back: Limits = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(back.cpu_max, Some(CpuMax::Max)));
}

#[test]
fn limits_partial_fields_round_trip() {
    let orig = Limits {
        cpu_max: None,
        memory_max: Some(64 * 1024 * 1024),
        pids_max: None,
    };
    let json = serde_json::to_string(&orig).expect("serialize");
    let back: Limits = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.memory_max, Some(64 * 1024 * 1024));
}
