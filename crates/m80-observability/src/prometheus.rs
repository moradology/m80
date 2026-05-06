use crate::{HealthSnapshot, ObservabilityError, OpsMetrics};

/// Render a Prometheus exposition-format text response.
pub fn render_prometheus(
    health: &HealthSnapshot,
    metrics: &OpsMetrics,
) -> Result<String, ObservabilityError> {
    let mut out = String::new();
    metric(&mut out, "m80_vm_health_healthy", health.healthy);
    metric(&mut out, "m80_vm_health_degraded", health.degraded);
    metric(&mut out, "m80_vm_health_stuck", health.stuck);
    metric(&mut out, "m80_vm_health_exited", health.exited);
    metric(&mut out, "m80_vm_health_total", health.total);
    metric(
        &mut out,
        "m80_vm_rollout_ready",
        u32::from(health.rollout_ready),
    );
    metric(&mut out, "m80_ops_vm_count", metrics.vm_count);
    Ok(out)
}

fn metric(out: &mut String, name: &str, value: u32) {
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_prometheus_text_contains_gauges_only() {
        let health = HealthSnapshot {
            healthy: 1,
            total: 1,
            rollout_ready: true,
            ..HealthSnapshot::default()
        };
        let metrics = OpsMetrics { vm_count: 1 };
        let rendered = render_prometheus(&health, &metrics).unwrap();
        assert!(rendered.contains("# TYPE m80_vm_health_healthy gauge"));
        assert!(rendered.contains("m80_vm_health_healthy 1\n"));
        assert!(rendered.contains("m80_vm_rollout_ready 1\n"));
        assert!(rendered.contains("m80_ops_vm_count 1\n"));
    }
}
