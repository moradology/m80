//! Metrics endpoint request-shape and typed-error tests.

mod fixture_server;
use fixture_server::{resp_204, resp_400, FixtureServer};

use std::path::PathBuf;

use m80_firecracker_client::{Client, ClientError, MetricsConfig};

#[test]
fn put_metrics_sends_firecracker_shape() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    client
        .put_metrics(&MetricsConfig {
            metrics_path: PathBuf::from("/firecracker-metrics.jsonl"),
        })
        .unwrap();

    let result = server.join();
    assert!(result.request.starts_with("PUT /metrics HTTP/1.1\r\n"));
    assert!(result
        .request
        .contains("\"metrics_path\":\"/firecracker-metrics.jsonl\""));
}

#[test]
fn metrics_config_serializes_exact_firecracker_shape() {
    let json = serde_json::to_string(&MetricsConfig {
        metrics_path: PathBuf::from("/firecracker-metrics.jsonl"),
    })
    .unwrap();

    assert_eq!(json, r#"{"metrics_path":"/firecracker-metrics.jsonl"}"#);
}

#[test]
fn put_metrics_400_returns_metrics_write_failed() {
    let server =
        FixtureServer::spawn(resp_400("{\"fault_message\":\"bad metrics path\"}")).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    let err = client
        .put_metrics(&MetricsConfig {
            metrics_path: PathBuf::from("/bad/metrics"),
        })
        .unwrap_err();

    server.join();
    assert!(
        matches!(err, ClientError::MetricsWriteFailed { ref fault } if fault.contains("bad metrics path")),
        "unexpected error: {err:?}"
    );
}
