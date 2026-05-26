use std::fmt::Write as _;

use crate::health::{
    HealthSnapshot, OpsMetrics, PmemSharingLabel, PostRestoreHookDuration, TemplateFreshnessLabel,
    WarmPoolMetrics,
};

const RESTORE_LATENCY_BUCKETS: &[HistogramBucket] = &[
    HistogramBucket {
        upper_bound_us: 50_000,
        le: "0.05",
    },
    HistogramBucket {
        upper_bound_us: 100_000,
        le: "0.1",
    },
    HistogramBucket {
        upper_bound_us: 150_000,
        le: "0.15",
    },
    HistogramBucket {
        upper_bound_us: 200_000,
        le: "0.2",
    },
    HistogramBucket {
        upper_bound_us: 500_000,
        le: "0.5",
    },
    HistogramBucket {
        upper_bound_us: 1_000_000,
        le: "1",
    },
];

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
    render_metric(
        &mut out,
        "m80_launches_total",
        "Successful VM launches observed by this process.",
        metrics.launches_total,
        "counter",
    );
    render_error_counts(&mut out, metrics);
    render_phase_failure_counts(&mut out, metrics);
    render_metric(
        &mut out,
        "m80_vsock_disconnects_total",
        "Vsock disconnects before a required terminal frame.",
        metrics.vsock_disconnects_total,
        "counter",
    );
    render_metric(
        &mut out,
        "m80_idle_timeout_total",
        "Idle-timeout expirations observed by the lifecycle watcher.",
        metrics.idle_timeout_total,
        "counter",
    );
    if let Some(warm_pool) = metrics.warm_pool {
        render_warm_pool_metrics(&mut out, warm_pool);
    }
    render_pmem_layer_count_by_sharing(&mut out, metrics);
    render_template_count_by_freshness(&mut out, metrics);
    render_histogram(
        &mut out,
        "m80_restore_latency_seconds",
        "Warm restore latency from template restore start to lease handback.",
        metrics.restore_latency_seconds.observations_us(),
        &[],
    );
    render_post_restore_hook_duration(&mut out, &metrics.post_restore_hook_duration_seconds);
    render_metric(
        &mut out,
        "m80_image_store_bytes",
        "Total bytes currently occupied by the m80 image store.",
        metrics.image_store_bytes,
        "gauge",
    );
    render_metric(
        &mut out,
        "m80_template_store_bytes",
        "Total bytes currently occupied by the m80 snapshot-template store.",
        metrics.template_store_bytes,
        "gauge",
    );
    render_lease_attribution(&mut out, metrics);
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

struct HistogramBucket {
    upper_bound_us: u64,
    le: &'static str,
}

fn render_metric(out: &mut String, name: &str, help: &str, value: u64, kind: &str) {
    render_family_header(out, name, help, kind);
    out.push_str(name);
    out.push(' ');
    write!(out, "{value}").unwrap();
    out.push('\n');
}

fn render_family_header(out: &mut String, name: &str, help: &str, kind: &str) {
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
}

fn render_pmem_layer_count_by_sharing(out: &mut String, metrics: &OpsMetrics) {
    let name = "m80_pmem_layers_per_vm_count";
    render_family_header(
        out,
        name,
        "Pmem layers per VM split by declared sharing mode.",
        "gauge",
    );
    render_labeled_sample(
        out,
        name,
        &[("sharing", PmemSharingLabel::PerVm.as_str())],
        metrics.pmem_layers_per_vm_count_by_sharing.per_vm,
    );
    render_labeled_sample(
        out,
        name,
        &[("sharing", PmemSharingLabel::Shared.as_str())],
        metrics.pmem_layers_per_vm_count_by_sharing.shared,
    );
}

fn render_template_count_by_freshness(out: &mut String, metrics: &OpsMetrics) {
    let name = "m80_template_count";
    render_family_header(
        out,
        name,
        "Snapshot templates split by freshness classification.",
        "gauge",
    );
    render_labeled_sample(
        out,
        name,
        &[("freshness", TemplateFreshnessLabel::Fresh.as_str())],
        metrics.template_count_by_freshness.fresh,
    );
    render_labeled_sample(
        out,
        name,
        &[("freshness", TemplateFreshnessLabel::Invalidated.as_str())],
        metrics.template_count_by_freshness.invalidated,
    );
}

fn render_error_counts(out: &mut String, metrics: &OpsMetrics) {
    let name = "m80_errors_total";
    render_family_header(
        out,
        name,
        "Errors observed by finite FcError variant name.",
        "counter",
    );
    for count in &metrics.errors_total {
        render_labeled_sample(
            out,
            name,
            &[("variant", count.variant.as_str())],
            count.total,
        );
    }
}

fn render_phase_failure_counts(out: &mut String, metrics: &OpsMetrics) {
    let name = "m80_phase_failures_total";
    render_family_header(
        out,
        name,
        "Failed launch phases observed by phase name.",
        "counter",
    );
    for count in &metrics.phase_failures_total {
        render_labeled_sample(out, name, &[("phase", count.phase.as_str())], count.total);
    }
}

fn render_warm_pool_metrics(out: &mut String, metrics: WarmPoolMetrics) {
    render_metric(
        out,
        "m80_warm_pool_target_ready",
        "Configured warm-pool ready-slot target.",
        metrics.target_ready,
        "gauge",
    );
    render_metric(
        out,
        "m80_warm_pool_ready",
        "Warm-pool slots ready to lease now.",
        metrics.ready,
        "gauge",
    );
    render_metric(
        out,
        "m80_warm_pool_filling",
        "Warm-pool slots currently being filled.",
        metrics.filling,
        "gauge",
    );
    render_metric(
        out,
        "m80_warm_pool_leased",
        "Warm-pool slots currently leased to callers.",
        metrics.leased,
        "gauge",
    );
    render_metric(
        out,
        "m80_warm_pool_discarded_total",
        "Warm-pool slots discarded since pool creation.",
        metrics.discarded_total,
        "counter",
    );
    render_metric(
        out,
        "m80_warm_pool_consecutive_fill_errors",
        "Consecutive warm-pool fill errors since the last successful fill.",
        metrics.consecutive_fill_errors,
        "gauge",
    );
    render_metric(
        out,
        "m80_warm_pool_fill_attempts_total",
        "Warm-pool slot-fill attempts since pool creation.",
        metrics.fill_attempts_total,
        "counter",
    );
    render_metric(
        out,
        "m80_warm_pool_fill_failures_total",
        "Warm-pool slot-fill failures since pool creation.",
        metrics.fill_failures_total,
        "counter",
    );
    render_metric(
        out,
        "m80_warm_pool_lease_acquired_total",
        "Warm-pool leases handed to callers since pool creation.",
        metrics.lease_acquired_total,
        "counter",
    );
    render_metric(
        out,
        "m80_warm_pool_lease_returned_total",
        "Warm-pool leases released by callers since pool creation.",
        metrics.lease_returned_total,
        "counter",
    );
}

fn render_post_restore_hook_duration(out: &mut String, metrics: &[PostRestoreHookDuration]) {
    let name = "m80_post_restore_hook_duration_seconds";
    render_family_header(
        out,
        name,
        "Post-restore hook execution duration split by closed HookSpec variant.",
        "histogram",
    );
    for metric in metrics {
        render_histogram_samples(
            out,
            name,
            metric.duration.observations_us(),
            &[("hook_variant", metric.hook_variant.as_str())],
        );
    }
}

fn render_lease_attribution(out: &mut String, metrics: &OpsMetrics) {
    let name = "m80_lease_attribution";
    render_family_header(
        out,
        name,
        "Info-style active lease attribution by template, pmem digest set, and scratch source.",
        "gauge",
    );
    for attribution in &metrics.lease_attribution {
        render_labeled_sample(
            out,
            name,
            &[
                (
                    "template_fingerprint",
                    attribution.template_fingerprint.as_str(),
                ),
                ("pmem_digest_set", attribution.pmem_digest_set.as_str()),
                ("scratch_source", attribution.scratch_source.as_str()),
            ],
            1,
        );
    }
}

fn render_histogram(
    out: &mut String,
    name: &str,
    help: &str,
    observations_us: &[u64],
    labels: &[(&str, &str)],
) {
    render_family_header(out, name, help, "histogram");
    render_histogram_samples(out, name, observations_us, labels);
}

fn render_histogram_samples(
    out: &mut String,
    name: &str,
    observations_us: &[u64],
    labels: &[(&str, &str)],
) {
    let bucket_name = format!("{name}_bucket");
    for bucket in RESTORE_LATENCY_BUCKETS {
        let count = observations_us
            .iter()
            .filter(|&&sample| sample <= bucket.upper_bound_us)
            .count();
        let mut bucket_labels = labels.to_vec();
        bucket_labels.push(("le", bucket.le));
        render_labeled_sample(out, &bucket_name, &bucket_labels, count);
    }
    let inf_count = observations_us.len();
    let mut inf_labels = labels.to_vec();
    inf_labels.push(("le", "+Inf"));
    render_labeled_sample(out, &bucket_name, &inf_labels, inf_count);

    let sum_us = observations_us.iter().copied().sum::<u64>();
    let sum_name = format!("{name}_sum");
    render_labeled_sample(out, &sum_name, labels, format_seconds(sum_us));

    let count_name = format!("{name}_count");
    render_labeled_sample(out, &count_name, labels, inf_count);
}

fn render_labeled_sample(
    out: &mut String,
    name: &str,
    labels: &[(&str, &str)],
    value: impl std::fmt::Display,
) {
    out.push_str(name);
    render_labels(out, labels);
    out.push(' ');
    write!(out, "{value}").unwrap();
    out.push('\n');
}

fn render_labels(out: &mut String, labels: &[(&str, &str)]) {
    if labels.is_empty() {
        return;
    }
    out.push('{');
    for (index, (name, value)) in labels.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(value);
        out.push('"');
    }
    out.push('}');
}

fn format_seconds(us: u64) -> String {
    format!("{:.6}", us as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use crate::health::DurationHistogram;

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

    #[test]
    fn restore_latency_histogram_has_target_buckets() {
        let rendered = render_prometheus(
            &HealthSnapshot::default(),
            &OpsMetrics {
                restore_latency_seconds: DurationHistogram::from_micros([42_000, 160_000]),
                ..OpsMetrics::default()
            },
        );

        assert!(rendered.contains("m80_restore_latency_seconds_bucket{le=\"0.05\"} 1\n"));
        assert!(rendered.contains("m80_restore_latency_seconds_bucket{le=\"0.2\"} 2\n"));
        assert!(rendered.contains("m80_restore_latency_seconds_bucket{le=\"+Inf\"} 2\n"));
        assert!(rendered.contains("m80_restore_latency_seconds_sum 0.202000\n"));
        assert!(rendered.contains("m80_restore_latency_seconds_count 2\n"));
    }

    #[test]
    fn render_prometheus_includes_production_counter_families() {
        let rendered = render_prometheus(
            &HealthSnapshot::default(),
            &OpsMetrics {
                launches_total: 3,
                errors_total: vec![crate::health::ErrorCount {
                    variant: crate::health::MetricLabelValue::new("Storage").unwrap(),
                    total: 2,
                }],
                phase_failures_total: vec![crate::health::PhaseFailureCount {
                    phase: crate::health::MetricLabelValue::new("phase_3_storage_prep").unwrap(),
                    total: 1,
                }],
                vsock_disconnects_total: 4,
                idle_timeout_total: 5,
                warm_pool: Some(WarmPoolMetrics {
                    target_ready: 2,
                    ready: 1,
                    filling: 1,
                    leased: 0,
                    discarded_total: 3,
                    consecutive_fill_errors: 1,
                    fill_attempts_total: 7,
                    fill_failures_total: 2,
                    lease_acquired_total: 5,
                    lease_returned_total: 4,
                }),
                ..OpsMetrics::default()
            },
        );

        assert!(rendered.contains("# TYPE m80_launches_total counter"));
        assert!(rendered.contains("m80_launches_total 3\n"));
        assert!(rendered.contains("m80_errors_total{variant=\"Storage\"} 2\n"));
        assert!(rendered.contains("m80_phase_failures_total{phase=\"phase_3_storage_prep\"} 1\n"));
        assert!(rendered.contains("m80_vsock_disconnects_total 4\n"));
        assert!(rendered.contains("m80_idle_timeout_total 5\n"));
        assert!(rendered.contains("m80_warm_pool_ready 1\n"));
        assert!(rendered.contains("m80_warm_pool_discarded_total 3\n"));
        assert!(rendered.contains("m80_warm_pool_fill_attempts_total 7\n"));
    }
}
