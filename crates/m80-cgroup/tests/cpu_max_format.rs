//! cpu.max file-format tests for the string `apply_limits` writes.

use m80_cgroup::CpuMax;

fn render_cpu_max(v: &CpuMax) -> String {
    match v {
        CpuMax::Quota {
            quota_us,
            period_us,
        } => format!("{quota_us} {period_us}\n"),
        CpuMax::Max => "max\n".to_owned(),
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
fn max_variant_formats_as_max_with_period() {
    assert_eq!(render_cpu_max(&CpuMax::Max), "max\n");
}
