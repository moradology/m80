use m80_observability::{render_prometheus, HealthSnapshot, OpsMetrics};
use m80_proto::{GuestCpuMetrics, GuestMemMetrics, MetricsResponse};

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

    let rendered = render_prometheus(&health, &metrics).unwrap();

    assert!(rendered.contains("# TYPE m80_guest_cpu_total_ticks counter"));
    assert!(rendered.contains("m80_guest_cpu_total_ticks 117\n"));
    assert!(rendered.contains("# TYPE m80_guest_mem_available_bytes gauge"));
    assert!(rendered.contains("m80_guest_mem_available_bytes 768\n"));
    assert!(rendered.contains("# TYPE m80_guest_requests_total counter"));
    assert!(rendered.contains("m80_guest_requests_total 7\n"));
}
