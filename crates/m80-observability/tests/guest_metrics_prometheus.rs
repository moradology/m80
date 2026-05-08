use m80_observability::{render_prometheus, HealthSnapshot, OpsMetrics};
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
    };
    let expected = expected_metrics();

    let rendered = render_prometheus(&health, &metrics);
    let mut help_names = BTreeSet::new();
    let mut type_names = BTreeMap::new();
    let mut sample_names = BTreeSet::new();

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
            let name = parts.next().expect("sample metric name");
            let value = parts.next().expect("sample metric value");
            assert!(
                parts.next().is_none(),
                "sample line must contain exactly name and value: {line:?}"
            );
            assert!(expected.contains_key(name), "unknown sample metric: {name}");
            value
                .parse::<u64>()
                .unwrap_or_else(|e| panic!("sample value for {name} must parse: {e}"));
            assert!(
                sample_names.insert(name.to_owned()),
                "duplicate sample for {name}"
            );
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
    assert_eq!(sample_names, expected_names);
}

fn expected_metrics() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("m80_vm_health_healthy", "gauge"),
        ("m80_vm_health_degraded", "gauge"),
        ("m80_vm_health_stuck", "gauge"),
        ("m80_vm_health_exited", "gauge"),
        ("m80_vm_health_total", "gauge"),
        ("m80_vm_rollout_ready", "gauge"),
        ("m80_ops_vm_count", "gauge"),
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
