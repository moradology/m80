use crate::{HealthSnapshot, OpsMetrics};

/// Render a Prometheus exposition-format text response.
pub fn render_prometheus(health: &HealthSnapshot, metrics: &OpsMetrics) -> String {
    let mut out = String::new();
    metric_u64(&mut out, "m80_vm_health_healthy", u64::from(health.healthy));
    metric_u64(
        &mut out,
        "m80_vm_health_degraded",
        u64::from(health.degraded),
    );
    metric_u64(&mut out, "m80_vm_health_stuck", u64::from(health.stuck));
    metric_u64(&mut out, "m80_vm_health_exited", u64::from(health.exited));
    metric_u64(&mut out, "m80_vm_health_total", u64::from(health.total));
    metric_u64(
        &mut out,
        "m80_vm_rollout_ready",
        u64::from(health.rollout_ready),
    );
    metric_u64(&mut out, "m80_ops_vm_count", u64::from(metrics.vm_count));
    if let Some(guest) = &metrics.guest {
        counter(&mut out, "m80_guest_cpu_total_ticks", guest.cpu.total_ticks);
        counter(&mut out, "m80_guest_cpu_user_ticks", guest.cpu.user_ticks);
        counter(&mut out, "m80_guest_cpu_nice_ticks", guest.cpu.nice_ticks);
        counter(
            &mut out,
            "m80_guest_cpu_system_ticks",
            guest.cpu.system_ticks,
        );
        counter(&mut out, "m80_guest_cpu_idle_ticks", guest.cpu.idle_ticks);
        counter(
            &mut out,
            "m80_guest_cpu_iowait_ticks",
            guest.cpu.iowait_ticks,
        );
        counter(&mut out, "m80_guest_cpu_irq_ticks", guest.cpu.irq_ticks);
        counter(
            &mut out,
            "m80_guest_cpu_softirq_ticks",
            guest.cpu.softirq_ticks,
        );
        counter(&mut out, "m80_guest_cpu_steal_ticks", guest.cpu.steal_ticks);
        counter(&mut out, "m80_guest_cpu_guest_ticks", guest.cpu.guest_ticks);
        counter(
            &mut out,
            "m80_guest_cpu_guest_nice_ticks",
            guest.cpu.guest_nice_ticks,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_total_bytes",
            guest.mem.mem_total_bytes,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_available_bytes",
            guest.mem.mem_available_bytes,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_free_bytes",
            guest.mem.mem_free_bytes,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_buffers_bytes",
            guest.mem.buffers_bytes,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_cached_bytes",
            guest.mem.cached_bytes,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_swap_total_bytes",
            guest.mem.swap_total_bytes,
        );
        metric_u64(
            &mut out,
            "m80_guest_mem_swap_free_bytes",
            guest.mem.swap_free_bytes,
        );
        counter(&mut out, "m80_guest_requests_total", guest.requests_total);
        counter(&mut out, "m80_guest_errors_total", guest.errors_total);
    }
    out
}

fn metric_u64(out: &mut String, name: &str, value: u64) {
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn counter(out: &mut String, name: &str, value: u64) {
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" counter\n");
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
