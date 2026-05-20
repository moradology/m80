use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) struct HttpFixture {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HttpFixture {
    pub(crate) fn new<const N: usize>(routes: [(&'static str, TestResponse); N]) -> Self {
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

    pub(crate) fn url(&self, path: &str) -> String {
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
pub(crate) enum TestResponse {
    Ok(Vec<u8>),
    Status {
        code: u16,
        body: Vec<u8>,
    },
    RedirectPlaceholderHost(String),
    Redirect(String),
    Truncated {
        body: Vec<u8>,
        advertised_len: usize,
    },
}

impl TestResponse {
    pub(crate) fn ok(body: Vec<u8>) -> Self {
        Self::Ok(body)
    }

    pub(crate) fn status(code: u16, body: Vec<u8>) -> Self {
        Self::Status { code, body }
    }

    pub(crate) fn redirect_placeholder_host(path: &str) -> Self {
        Self::RedirectPlaceholderHost(path.to_owned())
    }

    pub(crate) fn truncated(body: Vec<u8>, advertised_len: usize) -> Self {
        Self::Truncated {
            body,
            advertised_len,
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
            stream.write_all(response.as_bytes()).unwrap();
        }
        Some(TestResponse::Truncated {
            body,
            advertised_len,
        }) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {advertised_len}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
        Some(TestResponse::RedirectPlaceholderHost(_)) => unreachable!("placeholder resolved"),
        None => write_response(stream, 404, b"missing"),
    }
}

fn write_response(stream: &mut std::net::TcpStream, status: u16, body: &[u8]) {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
}
