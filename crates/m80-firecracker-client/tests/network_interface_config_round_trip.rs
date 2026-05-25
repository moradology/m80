//! Firecracker network-interface request shape.

mod fixture_server;
use fixture_server::{resp_204, resp_400, FixtureServer};

use m80_firecracker_client::{
    Client, ClientError, NetworkInterfaceConfig, RateLimiterConfig, TokenBucketConfig,
};

#[test]
fn put_network_interface_sends_tap_and_guest_mac() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    client
        .put_network_interface(&NetworkInterfaceConfig {
            iface_id: "eth0".to_owned(),
            host_dev_name: "tfc123456789abc".to_owned(),
            guest_mac: Some("02:00:00:00:00:02".to_owned()),
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        })
        .unwrap();

    let result = server.join();
    assert!(
        result
            .request
            .starts_with("PUT /network-interfaces/eth0 HTTP/1.1\r\n"),
        "iface_id must appear in URL: {}",
        result.request.lines().next().unwrap()
    );
    assert!(result.request.contains("\"iface_id\":\"eth0\""));
    assert!(result
        .request
        .contains("\"host_dev_name\":\"tfc123456789abc\""));
    assert!(result
        .request
        .contains("\"guest_mac\":\"02:00:00:00:00:02\""));
}

#[test]
fn put_network_interface_omits_absent_guest_mac() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    client
        .put_network_interface(&NetworkInterfaceConfig {
            iface_id: "eth0".to_owned(),
            host_dev_name: "tfc123456789abc".to_owned(),
            guest_mac: None,
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        })
        .unwrap();

    let result = server.join();
    assert!(
        !result.request.contains("\"guest_mac\""),
        "None guest_mac must be omitted"
    );
    assert!(
        !result.request.contains("\"rx_rate_limiter\""),
        "None rx_rate_limiter must be omitted"
    );
    assert!(
        !result.request.contains("\"tx_rate_limiter\""),
        "None tx_rate_limiter must be omitted"
    );
}

#[test]
fn put_network_interface_sends_rate_limiters_when_set() {
    let server = FixtureServer::spawn(resp_204()).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    client
        .put_network_interface(&NetworkInterfaceConfig {
            iface_id: "eth0".to_owned(),
            host_dev_name: "tfc123456789abc".to_owned(),
            guest_mac: None,
            rx_rate_limiter: Some(RateLimiterConfig {
                bandwidth: Some(TokenBucketConfig {
                    size: 1_048_576,
                    one_time_burst: Some(65_536),
                    refill_time: 1_000,
                }),
                ops: None,
            }),
            tx_rate_limiter: Some(RateLimiterConfig {
                bandwidth: None,
                ops: Some(TokenBucketConfig {
                    size: 1_024,
                    one_time_burst: None,
                    refill_time: 100,
                }),
            }),
        })
        .unwrap();

    let result = server.join();
    assert!(result.request.contains("\"rx_rate_limiter\""));
    assert!(result.request.contains("\"tx_rate_limiter\""));
    assert!(result.request.contains("\"bandwidth\""));
    assert!(result.request.contains("\"ops\""));
    assert!(result.request.contains("\"size\":1048576"));
    assert!(result.request.contains("\"one_time_burst\":65536"));
    assert!(result.request.contains("\"refill_time\":1000"));
    assert!(result.request.contains("\"size\":1024"));
    assert!(result.request.contains("\"refill_time\":100"));
}

#[test]
fn network_interface_400_returns_network_interface_write_failed() {
    let server = FixtureServer::spawn(resp_400("{\"fault_message\":\"bad tap\"}")).unwrap();
    let client = Client::new(&server.socket_path).unwrap();

    let err = client
        .put_network_interface(&NetworkInterfaceConfig {
            iface_id: "eth0".to_owned(),
            host_dev_name: "missing-tap".to_owned(),
            guest_mac: None,
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        })
        .unwrap_err();

    server.join();
    assert!(
        matches!(err, ClientError::NetworkInterfaceWriteFailed { ref fault } if fault.contains("bad tap")),
        "expected network interface error, got {err:?}"
    );
}
