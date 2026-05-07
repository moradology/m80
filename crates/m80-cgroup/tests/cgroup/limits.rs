use m80_cgroup::{CpuMax, IoMax, Limits};

#[test]
fn cpu_max_one_cpu() {
    let limits = Limits::m80_default();

    match limits.cpu_max.expect("default cpu limit") {
        CpuMax::Quota {
            quota_us,
            period_us,
        } => {
            assert_eq!(quota_us, 100_000);
            assert_eq!(period_us, 100_000);
        }
        CpuMax::Max => panic!("default must be a concrete one-cpu quota"),
    }
}

#[test]
fn memory_and_pids_max() {
    let limits = Limits::m80_default();

    assert_eq!(limits.memory_max, Some(1_610_612_736));
    assert_eq!(limits.pids_max, Some(128));
    assert_eq!(limits.io_weight, Some(100));
    assert_eq!(limits.oom_score_adj, Some(500));
    assert!(limits.io_max.is_empty());
}

#[test]
fn disabled_mode_skips() {
    let no_limits = Limits::default();

    assert!(no_limits.cpu_max.is_none());
    assert!(no_limits.memory_max.is_none());
    assert!(no_limits.pids_max.is_none());
    assert!(no_limits.io_max.is_empty());
    assert!(no_limits.io_weight.is_none());
    assert!(no_limits.oom_score_adj.is_none());
}

#[test]
fn io_max_json_round_trip() {
    let limit = IoMax {
        major: 8,
        minor: 0,
        rbps: Some(1_000_000),
        wbps: None,
        riops: None,
        wiops: Some(100),
    };
    let json = serde_json::to_string(&limit).unwrap();
    let back: IoMax = serde_json::from_str(&json).unwrap();
    assert_eq!(back, limit);
}
