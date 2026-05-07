use crate::{HealthSnapshot, OpsMetrics};

/// Render a Prometheus exposition-format text response.
pub fn render_prometheus(health: &HealthSnapshot, metrics: &OpsMetrics) -> String {
    let mut out = String::new();
    render_metric(&mut out, "m80_vm_health_healthy", u64::from(health.healthy), "gauge");
    render_metric(&mut out, "m80_vm_health_degraded", u64::from(health.degraded), "gauge");
    render_metric(&mut out, "m80_vm_health_stuck", u64::from(health.stuck), "gauge");
    render_metric(&mut out, "m80_vm_health_exited", u64::from(health.exited), "gauge");
    render_metric(&mut out, "m80_vm_health_total", u64::from(health.total), "gauge");
    render_metric(&mut out, "m80_vm_rollout_ready", u64::from(health.rollout_ready), "gauge");
    render_metric(&mut out, "m80_ops_vm_count", u64::from(metrics.vm_count), "gauge");
    if let Some(guest) = &metrics.guest {
        render_metric(&mut out, "m80_guest_cpu_total_ticks", guest.cpu.total_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_user_ticks", guest.cpu.user_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_nice_ticks", guest.cpu.nice_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_system_ticks", guest.cpu.system_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_idle_ticks", guest.cpu.idle_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_iowait_ticks", guest.cpu.iowait_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_irq_ticks", guest.cpu.irq_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_softirq_ticks", guest.cpu.softirq_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_steal_ticks", guest.cpu.steal_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_guest_ticks", guest.cpu.guest_ticks, "counter");
        render_metric(&mut out, "m80_guest_cpu_guest_nice_ticks", guest.cpu.guest_nice_ticks, "counter");
        render_metric(&mut out, "m80_guest_mem_total_bytes", guest.mem.mem_total_bytes, "gauge");
        render_metric(&mut out, "m80_guest_mem_available_bytes", guest.mem.mem_available_bytes, "gauge");
        render_metric(&mut out, "m80_guest_mem_free_bytes", guest.mem.mem_free_bytes, "gauge");
        render_metric(&mut out, "m80_guest_mem_buffers_bytes", guest.mem.buffers_bytes, "gauge");
        render_metric(&mut out, "m80_guest_mem_cached_bytes", guest.mem.cached_bytes, "gauge");
        render_metric(&mut out, "m80_guest_mem_swap_total_bytes", guest.mem.swap_total_bytes, "gauge");
        render_metric(&mut out, "m80_guest_mem_swap_free_bytes", guest.mem.swap_free_bytes, "gauge");
        render_metric(&mut out, "m80_guest_requests_total", guest.requests_total, "counter");
        render_metric(&mut out, "m80_guest_errors_total", guest.errors_total, "counter");
    }
    out
}

fn render_metric(out: &mut String, name: &str, value: u64, kind: &str) {
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push(' ');
    out.push_str(kind);
    out.push('\n');
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
        let metrics = OpsMetrics {
            vm_count: 1,
            ..OpsMetrics::default()
        };
        let rendered = render_prometheus(&health, &metrics);
        assert!(rendered.contains("# TYPE m80_vm_health_healthy gauge"));
        assert!(rendered.contains("m80_vm_health_healthy 1\n"));
        assert!(rendered.contains("m80_vm_rollout_ready 1\n"));
        assert!(rendered.contains("m80_ops_vm_count 1\n"));
    }
}
