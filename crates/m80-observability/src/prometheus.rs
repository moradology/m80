use crate::health::{HealthSnapshot, OpsMetrics};

/// Render a Prometheus exposition-format text response.
pub fn render_prometheus(health: &HealthSnapshot, metrics: &OpsMetrics) -> String {
    let mut out = String::new();
    render_metric(
        &mut out,
        "m80_vm_health_healthy",
        "Number of probed VMs classified as healthy.",
        u64::from(health.healthy),
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_vm_health_degraded",
        "Number of probed VMs classified as degraded.",
        u64::from(health.degraded),
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_vm_health_stuck",
        "Number of probed VMs classified as stuck.",
        u64::from(health.stuck),
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_vm_health_exited",
        "Number of probed VMs classified as exited.",
        u64::from(health.exited),
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_vm_health_total",
        "Total number of probed VMs.",
        u64::from(health.total),
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_vm_rollout_ready",
        "Whether all probed VMs are ready for rollout.",
        u64::from(health.rollout_ready),
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_ops_vm_count",
        "Number of VMs represented by operational metrics.",
        u64::from(metrics.vm_count),
        "gauge",
    );
    if let Some(guest) = &metrics.guest {
        render_metric(
            &mut out,
            "m80_guest_cpu_total_ticks",
            "Guest CPU total jiffies reported by /proc/stat.",
            guest.cpu.total_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_user_ticks",
            "Guest CPU user jiffies reported by /proc/stat.",
            guest.cpu.user_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_nice_ticks",
            "Guest CPU nice jiffies reported by /proc/stat.",
            guest.cpu.nice_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_system_ticks",
            "Guest CPU system jiffies reported by /proc/stat.",
            guest.cpu.system_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_idle_ticks",
            "Guest CPU idle jiffies reported by /proc/stat.",
            guest.cpu.idle_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_iowait_ticks",
            "Guest CPU iowait jiffies reported by /proc/stat.",
            guest.cpu.iowait_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_irq_ticks",
            "Guest CPU IRQ jiffies reported by /proc/stat.",
            guest.cpu.irq_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_softirq_ticks",
            "Guest CPU softirq jiffies reported by /proc/stat.",
            guest.cpu.softirq_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_steal_ticks",
            "Guest CPU steal jiffies reported by /proc/stat.",
            guest.cpu.steal_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_guest_ticks",
            "Guest CPU guest jiffies reported by /proc/stat.",
            guest.cpu.guest_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_cpu_guest_nice_ticks",
            "Guest CPU guest_nice jiffies reported by /proc/stat.",
            guest.cpu.guest_nice_ticks,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_total_bytes",
            "Guest total memory from /proc/meminfo.",
            guest.mem.mem_total_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_available_bytes",
            "Guest available memory from /proc/meminfo.",
            guest.mem.mem_available_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_free_bytes",
            "Guest free memory from /proc/meminfo.",
            guest.mem.mem_free_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_buffers_bytes",
            "Guest buffer memory from /proc/meminfo.",
            guest.mem.buffers_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_cached_bytes",
            "Guest cached memory from /proc/meminfo.",
            guest.mem.cached_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_swap_total_bytes",
            "Guest total swap from /proc/meminfo.",
            guest.mem.swap_total_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_mem_swap_free_bytes",
            "Guest free swap from /proc/meminfo.",
            guest.mem.swap_free_bytes,
            "gauge",
        );
        render_metric(
            &mut out,
            "m80_guest_requests_total",
            "Total guestd requests observed by the guest.",
            guest.requests_total,
            "counter",
        );
        render_metric(
            &mut out,
            "m80_guest_errors_total",
            "Total guestd errors observed by the guest.",
            guest.errors_total,
            "counter",
        );
    }
    out
}

fn render_metric(out: &mut String, name: &str, help: &str, value: u64, kind: &str) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
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
