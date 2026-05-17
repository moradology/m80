use m80_observability::{
    render_prometheus, DurationHistogram, HealthSnapshot, LeaseAttribution, MetricLabelValue,
    OpsMetrics, PmemLayerCountBySharing, PostRestoreHookDuration, PostRestoreHookVariantLabel,
    ScratchSourceLabel, TemplateCountByFreshness,
};
use m80_proto::{GuestCpuMetrics, GuestMemMetrics, MetricsResponse};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn prometheus_render_includes_guest_metrics() {
    let health = HealthSnapshot {
        healthy: 1,
        total: 1,
        rollout_ready: true,
        ..HealthSnapshot::default()
    };
    let metrics = OpsMetrics {
        vm_count: 1,
        guest: Some(MetricsResponse {
            cpu: GuestCpuMetrics {
                user_ticks: 10,
                nice_ticks: 0,
                system_ticks: 5,
                idle_ticks: 100,
                iowait_ticks: 1,
                irq_ticks: 0,
                softirq_ticks: 1,
                steal_ticks: 0,
                guest_ticks: 0,
                guest_nice_ticks: 0,
                total_ticks: 117,
            },
            mem: GuestMemMetrics {
                mem_total_bytes: 1024,
                mem_available_bytes: 768,
                mem_free_bytes: 512,
                buffers_bytes: 128,
                cached_bytes: 256,
                swap_total_bytes: 0,
                swap_free_bytes: 0,
            },
            requests_total: 7,
            errors_total: 1,
        }),
        ..OpsMetrics::default()
    };

    let rendered = render_prometheus(&health, &metrics);

    assert!(rendered.contains("# TYPE m80_guest_cpu_total_ticks counter"));
    assert!(rendered.contains("m80_guest_cpu_total_ticks 117\n"));
    assert!(rendered.contains("# TYPE m80_guest_mem_available_bytes gauge"));
    assert!(rendered.contains("m80_guest_mem_available_bytes 768\n"));
    assert!(rendered.contains("# TYPE m80_guest_requests_total counter"));
    assert!(rendered.contains("m80_guest_requests_total 7\n"));
}

#[test]
fn prometheus_render_spec_compliant() {
    let health = HealthSnapshot {
        healthy: 1,
        degraded: 2,
        stuck: 3,
        exited: 4,
        total: 10,
        rollout_ready: false,
    };
    let metrics = OpsMetrics {
        vm_count: 10,
        guest: Some(MetricsResponse {
            cpu: GuestCpuMetrics {
                user_ticks: 10,
                nice_ticks: 11,
                system_ticks: 12,
                idle_ticks: 13,
                iowait_ticks: 14,
                irq_ticks: 15,
                softirq_ticks: 16,
                steal_ticks: 17,
                guest_ticks: 18,
                guest_nice_ticks: 19,
                total_ticks: 145,
            },
            mem: GuestMemMetrics {
                mem_total_bytes: 1024,
                mem_available_bytes: 768,
                mem_free_bytes: 512,
                buffers_bytes: 128,
                cached_bytes: 256,
                swap_total_bytes: 64,
                swap_free_bytes: 32,
            },
            requests_total: 7,
            errors_total: 1,
        }),
        pmem_layers_per_vm_count_by_sharing: PmemLayerCountBySharing {
            per_vm: 3,
            shared: 2,
        },
        template_count_by_freshness: TemplateCountByFreshness {
            fresh: 4,
            invalidated: 1,
        },
        restore_latency_seconds: DurationHistogram::from_micros([42_000, 150_000, 210_000]),
        post_restore_hook_duration_seconds: vec![PostRestoreHookDuration {
            hook_variant: PostRestoreHookVariantLabel::SetHostname,
            duration: DurationHistogram::from_micros([2_000, 4_000]),
        }],
        image_store_bytes: 4096,
        template_store_bytes: 8192,
        lease_attribution: vec![LeaseAttribution {
            template_fingerprint: MetricLabelValue::new("sha256:template").unwrap(),
            pmem_digest_set: MetricLabelValue::new("sha256:pmem-a,sha256:pmem-b").unwrap(),
            scratch_source: ScratchSourceLabel::Workspace,
        }],
    };
    let expected = expected_metric_families();

    let rendered = render_prometheus(&health, &metrics);
    let mut help_names = BTreeSet::new();
    let mut type_names = BTreeMap::new();
    let mut sample_families = BTreeSet::new();

    for line in rendered.lines() {
        if let Some(rest) = line.strip_prefix("# HELP ") {
            let (name, help) = rest
                .split_once(' ')
                .unwrap_or_else(|| panic!("malformed HELP line: {line:?}"));
            assert!(!help.trim().is_empty(), "HELP text must not be empty");
            assert!(expected.contains_key(name), "unknown HELP metric: {name}");
            assert!(
                help_names.insert(name.to_owned()),
                "duplicate HELP for {name}"
            );
        } else if let Some(rest) = line.strip_prefix("# TYPE ") {
            let (name, kind) = rest
                .split_once(' ')
                .unwrap_or_else(|| panic!("malformed TYPE line: {line:?}"));
            assert_eq!(
                expected.get(name).copied(),
                Some(kind),
                "unexpected type for {name}"
            );
            assert!(
                type_names
                    .insert(name.to_owned(), kind.to_owned())
                    .is_none(),
                "duplicate TYPE for {name}"
            );
        } else {
            let mut parts = line.split_whitespace();
            let token = parts.next().expect("sample metric name");
            let value = parts.next().expect("sample metric value");
            assert!(
                parts.next().is_none(),
                "sample line must contain exactly name and value: {line:?}"
            );
            let family = sample_family(token, &expected);
            assert!(
                expected.contains_key(family),
                "unknown sample metric: {token}"
            );
            value
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("sample value for {token} must parse: {e}"));
            sample_families.insert(family.to_owned());
        }
    }

    let expected_names = expected
        .keys()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(help_names, expected_names);
    assert_eq!(
        type_names.keys().cloned().collect::<BTreeSet<_>>(),
        expected_names
    );
    assert_eq!(sample_families, expected_names);

    assert!(rendered.contains("m80_pmem_layers_per_vm_count{sharing=\"per_vm\"} 3\n"));
    assert!(rendered.contains("m80_pmem_layers_per_vm_count{sharing=\"shared\"} 2\n"));
    assert!(rendered.contains("m80_template_count{freshness=\"fresh\"} 4\n"));
    assert!(rendered.contains("m80_template_count{freshness=\"invalidated\"} 1\n"));
    assert!(rendered.contains("m80_restore_latency_seconds_bucket{le=\"0.2\"} 2\n"));
    assert!(rendered.contains(
        "m80_post_restore_hook_duration_seconds_bucket{hook_variant=\"set_hostname\",le=\"0.05\"} 2\n"
    ));
    assert!(rendered.contains("m80_image_store_bytes 4096\n"));
    assert!(rendered.contains("m80_template_store_bytes 8192\n"));
    assert!(rendered.contains(
        "m80_lease_attribution{template_fingerprint=\"sha256:template\",pmem_digest_set=\"sha256:pmem-a,sha256:pmem-b\",scratch_source=\"workspace\"} 1\n"
    ));
}

fn expected_metric_families() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("m80_vm_health_healthy", "gauge"),
        ("m80_vm_health_degraded", "gauge"),
        ("m80_vm_health_stuck", "gauge"),
        ("m80_vm_health_exited", "gauge"),
        ("m80_vm_health_total", "gauge"),
        ("m80_vm_rollout_ready", "gauge"),
        ("m80_ops_vm_count", "gauge"),
        ("m80_pmem_layers_per_vm_count", "gauge"),
        ("m80_template_count", "gauge"),
        ("m80_restore_latency_seconds", "histogram"),
        ("m80_post_restore_hook_duration_seconds", "histogram"),
        ("m80_image_store_bytes", "gauge"),
        ("m80_template_store_bytes", "gauge"),
        ("m80_lease_attribution", "gauge"),
        ("m80_guest_cpu_total_ticks", "counter"),
        ("m80_guest_cpu_user_ticks", "counter"),
        ("m80_guest_cpu_nice_ticks", "counter"),
        ("m80_guest_cpu_system_ticks", "counter"),
        ("m80_guest_cpu_idle_ticks", "counter"),
        ("m80_guest_cpu_iowait_ticks", "counter"),
        ("m80_guest_cpu_irq_ticks", "counter"),
        ("m80_guest_cpu_softirq_ticks", "counter"),
        ("m80_guest_cpu_steal_ticks", "counter"),
        ("m80_guest_cpu_guest_ticks", "counter"),
        ("m80_guest_cpu_guest_nice_ticks", "counter"),
        ("m80_guest_mem_total_bytes", "gauge"),
        ("m80_guest_mem_available_bytes", "gauge"),
        ("m80_guest_mem_free_bytes", "gauge"),
        ("m80_guest_mem_buffers_bytes", "gauge"),
        ("m80_guest_mem_cached_bytes", "gauge"),
        ("m80_guest_mem_swap_total_bytes", "gauge"),
        ("m80_guest_mem_swap_free_bytes", "gauge"),
        ("m80_guest_requests_total", "counter"),
        ("m80_guest_errors_total", "counter"),
    ])
}

fn sample_family<'a>(token: &'a str, expected: &BTreeMap<&'static str, &'static str>) -> &'a str {
    let name = token.split_once('{').map_or(token, |(name, _)| name);
    for suffix in ["_bucket", "_sum", "_count"] {
        if let Some(base) = name.strip_suffix(suffix) {
            if expected.get(base).copied() == Some("histogram") {
                return base;
            }
        }
    }
    name
}
