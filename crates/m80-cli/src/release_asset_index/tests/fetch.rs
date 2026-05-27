use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::super::fetch::{
    fetch_verified_asset_index, github_release_asset_index_url, sha256_bytes,
    AssetIndexDownloadBounds, AssetIndexFetchError, AssetIndexFetchRequest,
};
use super::super::{ASSET_INDEX_NAME, ASSET_INDEX_SCHEMA_VERSION};
use super::{asset_json, index_json, index_json_with_schema, linux_x86_64};
use crate::test_support::PROCESS_ENV_LOCK;

#[test]
fn verified_file_index_fetch_accepts_valid_integrity_digest_before_parse() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_integrity(
        temp.path(),
        &index_json_with_schema(ASSET_INDEX_SCHEMA_VERSION),
    );

    let fetched = fetch_verified_asset_index(fetch_request(&index_url)).unwrap();

    assert_eq!(fetched.index.release_tag, "v0.0.0");
    assert_eq!(fetched.index_url, index_url);
    assert_eq!(fetched.checksum_url, sibling_integrity_url(&index_url));
    assert_eq!(fetched.expected_sha256, fetched.observed_sha256);
    assert_eq!(fetched.index.assets[0].name, "m80-linux-x86_64.tar.gz");
}

#[test]
fn verified_file_index_fetch_names_missing_index_with_context() {
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join(ASSET_INDEX_NAME);
    let index_url = file_url(&index);

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    let message = err.to_string();
    assert!(message.contains(index.to_str().unwrap()), "{message}");
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(message.contains("requested_os=linux"), "{message}");
    assert!(message.contains("requested_arch=x86_64"), "{message}");
    assert!(
        message.contains("requested_image_kind=minimal"),
        "{message}"
    );
    assert!(
        message.contains("checksum_verification=before"),
        "{message}"
    );
}

#[test]
fn verified_file_index_fetch_names_missing_integrity_material() {
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join(ASSET_INDEX_NAME);
    fs::write(&index, index_json_with_schema(ASSET_INDEX_SCHEMA_VERSION)).unwrap();
    let index_url = file_url(&index);

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    let message = err.to_string();
    assert!(message.contains("m80-release-integrity.json"), "{message}");
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(
        message.contains("checksum_verification=before"),
        "{message}"
    );
}

#[test]
fn verified_file_index_fetch_rejects_bad_integrity_digest_before_json_parse() {
    let temp = tempfile::tempdir().unwrap();
    let index = temp.path().join(ASSET_INDEX_NAME);
    fs::write(&index, "{not json").unwrap();
    fs::write(
        temp.path().join("m80-release-integrity.json"),
        integrity_json("v0.0.0", &"0".repeat(64)),
    )
    .unwrap();
    let index_url = file_url(&index);

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    assert!(matches!(err, AssetIndexFetchError::ChecksumMismatch { .. }));
    let message = err.to_string();
    assert!(message.contains("expected"), "{message}");
    assert!(message.contains("observed"), "{message}");
    assert!(message.contains("index_url=file://"), "{message}");
    assert!(message.contains("checksum_verification=after"), "{message}");
    assert!(!message.contains("JSON is invalid"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_invalid_json_after_valid_integrity_digest() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_integrity(temp.path(), "{not json");

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexFetchError::VerifiedIndexInvalid { .. }
    ));
    let message = err.to_string();
    assert!(
        message.contains("verified release asset index invalid"),
        "{message}"
    );
    assert!(message.contains("expected_sha256="), "{message}");
    assert!(message.contains("observed_sha256="), "{message}");
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(message.contains("checksum_verification=after"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_stale_schema_after_valid_integrity_digest() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_integrity(temp.path(), &index_json_with_schema(999));

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("schema mismatch: expected 2, got 999"),
        "{message}"
    );
    assert!(message.contains("expected_sha256="), "{message}");
    assert!(message.contains("requested_os=linux"), "{message}");
    assert!(message.contains("requested_arch=x86_64"), "{message}");
    assert!(message.contains("checksum_verification=after"), "{message}");
}

#[test]
fn verified_file_index_fetch_rejects_index_release_tag_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let index_url = write_index_with_integrity(
        temp.path(),
        &index_json(
            "v9.9.9",
            asset_json("linux", "x86_64", "minimal", "v9.9.9", "v9.9.9"),
        ),
    );

    let err = fetch_verified_asset_index(fetch_request(&index_url)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexFetchError::ReleaseTagMismatch { .. }
    ));
    let message = err.to_string();
    assert!(message.contains("expected release_tag v0.0.0"), "{message}");
    assert!(message.contains("got v9.9.9"), "{message}");
    assert!(message.contains("observed_sha256="), "{message}");
    assert!(message.contains("checksum_verification=after"), "{message}");
}

#[test]
fn remote_index_fetch_times_out_with_bounded_context() {
    let _guard = PROCESS_ENV_LOCK.lock().expect("process env lock poisoned");
    let server = HttpFixture::new([(
        "/m80-release-assets.json",
        TestResponse::slow_ok(index_json_with_schema(ASSET_INDEX_SCHEMA_VERSION).into_bytes()),
    )]);
    let index_url = server.url("/m80-release-assets.json");

    let err = fetch_verified_asset_index(timeout_fetch_request(&index_url)).unwrap_err();

    assert!(matches!(err, AssetIndexFetchError::DownloadFailed { .. }));
    let diagnostic = err.clone().into_diagnostic("v0.0.0");
    assert_eq!(diagnostic.index_url.as_deref(), Some(index_url.as_str()));
    assert_eq!(diagnostic.fetch_url.as_deref(), Some(index_url.as_str()));
    assert_eq!(diagnostic.checksum_verification.as_deref(), Some("before"));
    let message = err.to_string();
    assert!(message.contains("failure=timeout"), "{message}");
    assert_fetch_context(&message, &index_url, "before");
}

#[test]
fn remote_checksum_fetch_times_out_with_bounded_context() {
    let _guard = PROCESS_ENV_LOCK.lock().expect("process env lock poisoned");
    let index = index_json_with_schema(ASSET_INDEX_SCHEMA_VERSION).into_bytes();
    let checksum = integrity_json("v0.0.0", &sha256_bytes(&index)).into_bytes();
    let server = HttpFixture::new([
        ("/m80-release-assets.json", TestResponse::ok(index)),
        (
            "/m80-release-integrity.json",
            TestResponse::slow_ok(checksum),
        ),
    ]);
    let index_url = server.url("/m80-release-assets.json");

    let err = fetch_verified_asset_index(timeout_fetch_request(&index_url)).unwrap_err();

    assert!(matches!(err, AssetIndexFetchError::DownloadFailed { .. }));
    let checksum_url = sibling_integrity_url(&index_url);
    let diagnostic = err.clone().into_diagnostic("v0.0.0");
    assert_eq!(diagnostic.index_url.as_deref(), Some(index_url.as_str()));
    assert_eq!(diagnostic.fetch_url.as_deref(), Some(checksum_url.as_str()));
    assert_eq!(diagnostic.checksum_verification.as_deref(), Some("before"));
    let message = err.to_string();
    assert!(message.contains("failure=timeout"), "{message}");
    assert!(message.contains("m80-release-integrity.json"), "{message}");
    assert_fetch_context(&message, &index_url, "before");
}

#[test]
fn remote_index_fetch_names_http_failure_context() {
    let _guard = PROCESS_ENV_LOCK.lock().expect("process env lock poisoned");
    let server = HttpFixture::new([(
        "/m80-release-assets.json",
        TestResponse::status(500, b"server failed".to_vec()),
    )]);
    let index_url = server.url("/m80-release-assets.json");

    let err = fetch_verified_asset_index(timeout_fetch_request(&index_url)).unwrap_err();

    assert!(matches!(err, AssetIndexFetchError::DownloadFailed { .. }));
    let message = err.to_string();
    assert!(message.contains("failure=http_failure"), "{message}");
    assert_fetch_context(&message, &index_url, "before");
}

#[test]
fn remote_index_fetch_names_connect_failure_context() {
    let _guard = PROCESS_ENV_LOCK.lock().expect("process env lock poisoned");
    let index_url = unused_local_fixture_url();

    let err = fetch_verified_asset_index(timeout_fetch_request(&index_url)).unwrap_err();

    assert!(matches!(err, AssetIndexFetchError::DownloadFailed { .. }));
    let message = err.to_string();
    assert!(message.contains("failure=connect_failure"), "{message}");
    assert_fetch_context(&message, &index_url, "before");
}

#[test]
fn remote_index_fetch_rejects_unsupported_redirect_with_context() {
    let _guard = PROCESS_ENV_LOCK.lock().expect("process env lock poisoned");
    let server = HttpFixture::new([
        (
            "/m80-release-assets.json",
            TestResponse::redirect_placeholder_host("/redirected-index"),
        ),
        (
            "/redirected-index",
            TestResponse::ok(index_json_with_schema(ASSET_INDEX_SCHEMA_VERSION).into_bytes()),
        ),
    ]);
    let index_url = server.url("/m80-release-assets.json");

    let err = fetch_verified_asset_index(timeout_fetch_request(&index_url)).unwrap_err();

    assert!(matches!(
        err,
        AssetIndexFetchError::RedirectUnsupported { .. }
    ));
    let message = err.to_string();
    assert!(
        message.contains("redirected to unsupported host"),
        "{message}"
    );
    assert_fetch_context(&message, &index_url, "before");
}

#[test]
fn pinned_github_asset_index_url_uses_moradology_m80_release() {
    assert_eq!(
        github_release_asset_index_url("v1.2.3"),
        "https://github.com/moradology/m80/releases/download/v1.2.3/m80-release-assets.json"
    );
}

fn fetch_request(index_url: &str) -> AssetIndexFetchRequest<'_> {
    fetch_request_with_bounds(index_url, AssetIndexDownloadBounds::default())
}

fn timeout_fetch_request(index_url: &str) -> AssetIndexFetchRequest<'_> {
    fetch_request_with_bounds(index_url, AssetIndexDownloadBounds::for_test(1, 1))
}

fn fetch_request_with_bounds(
    index_url: &str,
    download_bounds: AssetIndexDownloadBounds,
) -> AssetIndexFetchRequest<'_> {
    AssetIndexFetchRequest {
        index_url,
        release_tag: "v0.0.0",
        host: linux_x86_64(),
        image_kind: Some("minimal"),
        download_bounds,
    }
}

fn write_index_with_integrity(root: &Path, json: &str) -> String {
    let index = root.join(ASSET_INDEX_NAME);
    fs::write(&index, json).unwrap();
    fs::write(
        root.join("m80-release-integrity.json"),
        integrity_json("v0.0.0", &sha256_bytes(json.as_bytes())),
    )
    .unwrap();
    file_url(&index)
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn sibling_integrity_url(index_url: &str) -> String {
    let (prefix, _) = index_url.rsplit_once('/').unwrap();
    format!("{prefix}/m80-release-integrity.json")
}

fn integrity_json(release_tag: &str, index_sha256: &str) -> String {
    format!(
        r#"{{
  "release_tag": "{release_tag}",
  "subjects": [
    {{
      "name": "{ASSET_INDEX_NAME}",
      "kind": "asset-index",
      "sha256": "{index_sha256}",
      "size_bytes": 1
    }}
  ]
}}"#
    )
}

fn assert_fetch_context(message: &str, index_url: &str, checksum_verification: &str) {
    assert!(message.contains("release_tag=v0.0.0"), "{message}");
    assert!(message.contains("requested_os=linux"), "{message}");
    assert!(message.contains("requested_arch=x86_64"), "{message}");
    assert!(
        message.contains("requested_image_kind=minimal"),
        "{message}"
    );
    assert!(
        message.contains(&format!("index_url={index_url}")),
        "{message}"
    );
    assert!(
        message.contains(&format!("checksum_verification={checksum_verification}")),
        "{message}"
    );
}

fn unused_local_fixture_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/m80-release-assets.json")
}

struct HttpFixture {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HttpFixture {
    fn new<const N: usize>(routes: [(&'static str, TestResponse); N]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let route_map = Arc::new(Mutex::new(
            routes
                .into_iter()
                .map(|(path, response)| (path.to_owned(), response.with_addr(addr)))
                .collect::<HashMap<_, _>>(),
        ));
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let routes = route_map.lock().unwrap();
                        handle_http_request(&mut stream, &routes);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            addr,
            shutdown,
            handle: Some(handle),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
    }
}

#[derive(Clone)]
enum TestResponse {
    Ok(Vec<u8>),
    Status { code: u16, body: Vec<u8> },
    RedirectPlaceholderHost(String),
    Redirect(String),
    SlowOk { body: Vec<u8>, delay: Duration },
}

impl TestResponse {
    fn ok(body: Vec<u8>) -> Self {
        Self::Ok(body)
    }

    fn status(code: u16, body: Vec<u8>) -> Self {
        Self::Status { code, body }
    }

    fn redirect_placeholder_host(path: &str) -> Self {
        Self::RedirectPlaceholderHost(path.to_owned())
    }

    fn slow_ok(body: Vec<u8>) -> Self {
        Self::SlowOk {
            body,
            delay: Duration::from_secs(2),
        }
    }

    fn with_addr(self, addr: SocketAddr) -> Self {
        match self {
            Self::RedirectPlaceholderHost(path) => {
                Self::Redirect(format!("http://localhost:{}{path}", addr.port()))
            }
            other => other,
        }
    }
}

fn handle_http_request(stream: &mut std::net::TcpStream, routes: &HashMap<String, TestResponse>) {
    let mut request = [0_u8; 2048];
    let Ok(nread) = stream.read(&mut request) else {
        return;
    };
    let request = String::from_utf8_lossy(&request[..nread]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    match routes.get(path) {
        Some(TestResponse::Ok(body)) => write_response(stream, 200, body),
        Some(TestResponse::Status { code, body }) => write_response(stream, *code, body),
        Some(TestResponse::Redirect(location)) => {
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
        }
        Some(TestResponse::SlowOk { body, delay }) => {
            thread::sleep(*delay);
            write_response(stream, 200, body);
        }
        Some(TestResponse::RedirectPlaceholderHost(_)) => unreachable!("placeholder resolved"),
        None => write_response(stream, 404, b"missing"),
    }
}

fn write_response(stream: &mut std::net::TcpStream, status: u16, body: &[u8]) {
    let reason = if status == 200 { "OK" } else { "Error" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}
