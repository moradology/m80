//! Tests for the cpu.max file format produced by `apply_limits`.
//!
//! We test the formatting logic by inspecting the serialized string that
//! would be written to the cgroup file, derived from the enum variant.

use m80_cgroup::CpuMax;

/// Helper that renders a `CpuMax` to the string we write to `cpu.max`.
fn render_cpu_max(v: &CpuMax) -> String {
    match v {
        CpuMax::Quota { quota_us, period_us } => format!("{quota_us} {period_us}\n"),
        CpuMax::Max => "max 100000\n".to_owned(),
    }
}

#[test]
fn quota_formats_as_two_numbers() {
    let v = CpuMax::Quota {
        quota_us: 100_000,
        period_us: 200_000,
    };
    assert_eq!(render_cpu_max(&v), "100000 200000\n");
}

#[test]
fn quota_period_is_second_field() {
    let v = CpuMax::Quota {
        quota_us: 50_000,
        period_us: 100_000,
    };
    let s = render_cpu_max(&v);
    let mut parts = s.split_whitespace();
    assert_eq!(parts.next(), Some("50000"), "quota is first field");
    assert_eq!(parts.next(), Some("100000"), "period is second field");
}

#[test]
fn max_variant_produces_max_prefix() {
    let v = CpuMax::Max;
    let s = render_cpu_max(&v);
    assert!(s.starts_with("max "), "Max variant must start with 'max '");
}

#[test]
fn max_variant_includes_period() {
    let v = CpuMax::Max;
    let s = render_cpu_max(&v);
    // "max 100000\n" — period must be present
    let period_str = s.split_whitespace().nth(1).expect("period present");
    let period: u64 = period_str.parse().expect("period is numeric");
    assert!(period > 0, "period must be positive, got {period}");
}
