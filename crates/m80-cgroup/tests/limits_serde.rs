//! JSON round-trip tests for `Limits` and `CpuMax`.

use m80_cgroup::{CpuMax, Limits};

#[test]
fn limits_default_round_trips() {
    let orig = Limits::default();
    let json = serde_json::to_string(&orig).expect("serialize");
    let back: Limits = serde_json::from_str(&json).expect("deserialize");
    assert!(back.cpu_max.is_none());
    assert!(back.memory_max.is_none());
    assert!(back.pids_max.is_none());
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

    assert!(back.memory_max == Some(1_610_612_736));
    assert!(back.pids_max == Some(128));
    let cpu = back.cpu_max.expect("cpu_max present");
    match cpu {
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
fn limits_none_fields_serialize_as_null() {
    let orig = Limits {
        cpu_max: None,
        memory_max: Some(64 * 1024 * 1024),
        pids_max: None,
    };
    let json = serde_json::to_string(&orig).expect("serialize");
    let back: Limits = serde_json::from_str(&json).expect("deserialize");
    assert!(back.cpu_max.is_none());
    assert_eq!(back.memory_max, Some(64 * 1024 * 1024));
    assert!(back.pids_max.is_none());
}
