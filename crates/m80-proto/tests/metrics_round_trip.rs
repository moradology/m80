use m80_proto::{
    read_frame, write_frame, Envelope, GuestCpuMetrics, GuestMemMetrics, MetricsRequest,
    MetricsResponse, PAYLOAD_KIND_METRICS_REQUEST, PAYLOAD_KIND_METRICS_RESPONSE,
};

#[test]
fn metrics_request_envelope_uses_metrics_kind() {
    let env = Envelope::with_request_id(MetricsRequest {}, "req-metrics".to_owned());

    assert_eq!(env.kind, PAYLOAD_KIND_METRICS_REQUEST);
    assert_eq!(env.request_id.as_deref(), Some("req-metrics"));
}

#[test]
fn metrics_response_round_trips_as_fixed_shape_payload() {
    let response = MetricsResponse {
        cpu: GuestCpuMetrics {
            user_ticks: 1,
            nice_ticks: 2,
            system_ticks: 3,
            idle_ticks: 4,
            iowait_ticks: 5,
            irq_ticks: 6,
            softirq_ticks: 7,
            steal_ticks: 8,
            guest_ticks: 9,
            guest_nice_ticks: 10,
            total_ticks: 55,
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
        requests_total: 11,
        errors_total: 1,
    };
    let env = Envelope::new(response.clone());
    let mut bytes = Vec::new();

    write_frame(&mut bytes, &env).unwrap();
    let round_trip: Envelope<MetricsResponse> = read_frame(&mut bytes.as_slice()).unwrap();

    assert_eq!(round_trip.kind, PAYLOAD_KIND_METRICS_RESPONSE);
    assert_eq!(round_trip.payload, response);
}
