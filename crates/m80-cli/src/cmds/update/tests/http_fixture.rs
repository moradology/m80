use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(super) struct HttpFixture {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, TestResponse>>>,
    requests: Arc<Mutex<Vec<String>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl HttpFixture {
    pub(super) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind HTTP fixture");
        listener
            .set_nonblocking(true)
            .expect("set HTTP fixture nonblocking");
        let addr = listener.local_addr().expect("read HTTP fixture addr");
        let routes = Arc::new(Mutex::new(HashMap::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_requests = Arc::clone(&requests);
        let thread_routes = Arc::clone(&routes);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let routes = thread_routes.lock().expect("lock HTTP fixture routes");
                        handle_http_request(&mut stream, &routes, &thread_requests);
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
            routes,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    pub(super) fn add_ok(&self, path: &str, body: Vec<u8>) {
        self.routes
            .lock()
            .expect("lock HTTP fixture routes")
            .insert(path.to_owned(), TestResponse::Ok(body));
    }

    pub(super) fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub(super) fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("lock HTTP fixture requests")
            .clone()
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join HTTP fixture");
        }
    }
}

enum TestResponse {
    Ok(Vec<u8>),
}

fn handle_http_request(
    stream: &mut std::net::TcpStream,
    routes: &HashMap<String, TestResponse>,
    requests: &Arc<Mutex<Vec<String>>>,
) {
    let mut request = [0_u8; 2048];
    let Ok(nread) = stream.read(&mut request) else {
        return;
    };
    if nread == 0 {
        return;
    }
    let request = String::from_utf8_lossy(&request[..nread]);
    let Some(path) = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
    else {
        return;
    };
    let path = path.to_owned();
    requests
        .lock()
        .expect("lock HTTP fixture requests")
        .push(path.clone());
    match routes.get(&path) {
        Some(TestResponse::Ok(body)) => write_response(stream, 200, body),
        None => write_response(stream, 500, b"unexpected update-check URL"),
    }
}

fn write_response(stream: &mut std::net::TcpStream, status: u16, body: &[u8]) {
    let reason = if status == 200 { "OK" } else { "Unexpected" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).expect("write header");
    stream.write_all(body).expect("write body");
}
