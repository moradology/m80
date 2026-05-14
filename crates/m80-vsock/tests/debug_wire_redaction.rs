use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use m80_proto::{Envelope, ExecRequest, GUEST_PORT_DEFAULT};
use m80_vsock::Channel;
use tempfile::TempDir;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Metadata, Subscriber};

struct CaptureSubscriber {
    events: Arc<Mutex<Vec<String>>>,
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(visitor.out);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
}

#[derive(Default)]
struct EventVisitor {
    out: String,
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        write!(self.out, "{}={value:?};", field.name()).expect("write debug field");
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        write!(self.out, "{}={value};", field.name()).expect("write string field");
    }
}

#[test]
fn debug_wire_redacts_exec_request_env_values() {
    std::env::set_var("M80_DEBUG_WIRE", "vsock");

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&path).expect("bind listener");

    let server = std::thread::spawn(move || {
        let mut reader = accept_and_handshake(&listener);
        let received: Envelope<ExecRequest> =
            m80_proto::read_frame(&mut reader).expect("read exec request");
        assert_eq!(
            received.payload.env,
            Some(vec![(
                "API_KEY".to_owned(),
                "sentinel-secret-for-debug-wire".to_owned()
            )])
        );
    });

    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = CaptureSubscriber {
        events: Arc::clone(&events),
    };

    tracing::subscriber::with_default(subscriber, || {
        let mut channel = Channel::open_uds_only(&path, GUEST_PORT_DEFAULT).expect("open channel");
        channel
            .send(&Envelope::new(ExecRequest {
                program: "/bin/sh".to_owned(),
                args: vec!["-lc".to_owned(), "true".to_owned()],
                cwd: None,
                env: Some(vec![(
                    "API_KEY".to_owned(),
                    "sentinel-secret-for-debug-wire".to_owned(),
                )]),
                stdin: None,
                timeout_ms: None,
                streaming: false,
            }))
            .expect("send request");
    });

    server.join().expect("server thread");

    let joined = events.lock().expect("events mutex poisoned").join("\n");
    assert!(!joined.contains("sentinel-secret-for-debug-wire"));
    assert!(joined.contains("env=[1 entries redacted]"));
}

fn accept_and_handshake(listener: &UnixListener) -> BufReader<UnixStream> {
    let (stream, _) = listener.accept().expect("accept");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read connect");
    assert_eq!(line, format!("CONNECT {GUEST_PORT_DEFAULT}\n"));
    reader.get_mut().write_all(b"OK 9001\n").expect("write ack");
    reader
}
