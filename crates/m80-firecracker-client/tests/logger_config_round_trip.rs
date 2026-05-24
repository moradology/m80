//! Logger endpoint request-shape and typed-error tests.

mod fixture_server;
use fixture_server::{resp_204, resp_400, FixtureServer};

use std::path::PathBuf;

use m80_firecracker_client::{Client, ClientError, LogLevel, LoggerConfig};

#[test]
fn put_logger_sends_firecracker_shape() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    client
        .put_logger(&LoggerConfig {
            log_path: PathBuf::from("/firecracker.log"),
            level: Some(LogLevel::Warning),
            show_level: Some(true),
            show_log_origin: Some(true),
        })
        .unwrap();

    let result = server.join();
    assert!(result.request.starts_with("PUT /logger HTTP/1.1\r\n"));
    assert!(result.request.contains("\"log_path\":\"/firecracker.log\""));
    assert!(result.request.contains("\"level\":\"Warning\""));
    assert!(result.request.contains("\"show_level\":true"));
    assert!(result.request.contains("\"show_log_origin\":true"));
}

#[test]
fn logger_config_omits_absent_optional_fields() {
    let json = serde_json::to_string(&LoggerConfig {
        log_path: PathBuf::from("/firecracker.log"),
        level: None,
        show_level: None,
        show_log_origin: None,
    })
    .unwrap();

    assert_eq!(json, r#"{"log_path":"/firecracker.log"}"#);
}

#[test]
fn put_logger_400_returns_logger_write_failed() {
    let server = FixtureServer::spawn(resp_400("{\"fault_message\":\"bad log path\"}")).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    let err = client
        .put_logger(&LoggerConfig {
            log_path: PathBuf::from("/bad/log"),
            level: Some(LogLevel::Warning),
            show_level: None,
            show_log_origin: None,
        })
        .unwrap_err();

    server.join();
    assert!(
        matches!(err, ClientError::LoggerWriteFailed { ref fault } if fault.contains("bad log path")),
        "unexpected error: {err:?}"
    );
}
