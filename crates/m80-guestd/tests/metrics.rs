use std::io::Cursor;

use m80_proto::{
    read_frame, write_frame, Envelope, MetricsRequest, MetricsResponse,
    PAYLOAD_KIND_METRICS_RESPONSE,
};

fn metrics_frame(request_id: &str) -> Vec<u8> {
    let env = Envelope::with_request_id(MetricsRequest {}, request_id.to_owned());
    let mut buf = Vec::new();
    write_frame(&mut buf, &env).expect("write_frame in test");
    buf
}

fn run_handler(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection(
        std::io::BufReader::new(Cursor::new(input)),
        &mut out,
    )
    .expect("handle_connection failed");
    out
}

#[test]
fn metrics_request_returns_guest_procfs_snapshot() {
    let out = run_handler(metrics_frame("req-metrics"));
    let mut cursor = Cursor::new(out);
    let response: Envelope<MetricsResponse> = read_frame(&mut cursor).unwrap();

    assert_eq!(response.kind, PAYLOAD_KIND_METRICS_RESPONSE);
    assert_eq!(response.request_id.as_deref(), Some("req-metrics"));
    assert!(
        response.payload.cpu.total_ticks > 0,
        "synthetic handler run should read non-zero aggregate CPU ticks"
    );
    assert!(
        response.payload.mem.mem_total_bytes > 0,
        "synthetic handler run should read non-zero guest memory"
    );
    assert!(response.payload.requests_total > 0);
}
