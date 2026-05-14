//! Microbench the protobuf frame send paths affected by m80-jp6ik.33.

use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use m80_proto::{write_frame, Envelope, ExecRequest, ExecStdout};

const STREAMING_FRAMES: usize = 100;
const CHUNK_BYTES: usize = 4096;

fn main() {
    let n = env_usize("N", 10_000);
    let started_at = unix_timestamp();

    let exec_send = bench_single_exec_send(n);
    let streaming_burst = bench_streaming_burst(n);

    println!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"bench\": \"vsock_proto_microcuts\",\n",
            "  \"started_at_unix\": {started_at},\n",
            "  \"n\": {n},\n",
            "  \"streaming_frames_per_sample\": {streaming_frames},\n",
            "  \"chunk_bytes\": {chunk_bytes},\n",
            "  \"single_exec_send_mean_ns\": {exec_mean_ns},\n",
            "  \"streaming_burst_100_frames_mean_ns\": {stream_mean_ns},\n",
            "  \"streaming_burst_100_frames_mean_us\": {stream_mean_us:.3}\n",
            "}}"
        ),
        started_at = started_at,
        n = n,
        streaming_frames = STREAMING_FRAMES,
        chunk_bytes = CHUNK_BYTES,
        exec_mean_ns = exec_send.mean_ns,
        stream_mean_ns = streaming_burst.mean_ns,
        stream_mean_us = streaming_burst.mean_ns as f64 / 1_000.0,
    );
    eprintln!(
        "vsock-proto microcuts: n={n} single_exec_send_mean={}ns streaming_100_frames_mean={:.3}us",
        exec_send.mean_ns,
        streaming_burst.mean_ns as f64 / 1_000.0
    );
}

fn bench_single_exec_send(n: usize) -> BenchStats {
    let envelope = Envelope::new(ExecRequest {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "printf m80-proto-microcut-single-exec-send".into(),
        ],
        cwd: Some("/tmp".into()),
        env: Some(vec![
            ("M80_REQUEST_LABEL".into(), "single-exec-send".into()),
            ("M80_LONGISH_VALUE".into(), "x".repeat(60)),
        ]),
        stdin: Some(vec![b'x'; 256]),
        timeout_ms: Some(5_000),
        streaming: false,
    });
    bench_writes(n, |stream| write_frame(stream, &envelope))
}

fn bench_streaming_burst(n: usize) -> BenchStats {
    let chunk = vec![b'x'; CHUNK_BYTES];
    bench_writes(n, |stream| {
        for seq in 0..STREAMING_FRAMES {
            let envelope = Envelope::new(ExecStdout {
                seq: seq as u32,
                bytes: chunk.clone(),
            });
            write_frame(stream, &envelope)?;
        }
        Ok(())
    })
}

fn bench_writes<F>(n: usize, mut f: F) -> BenchStats
where
    F: FnMut(&mut UnixStream) -> Result<(), m80_proto::ProtoError>,
{
    assert!(n > 0, "N must be greater than zero");
    let (mut writer, reader) = UnixStream::pair().expect("UnixStream::pair");
    let reader = thread::spawn(move || drain(reader));

    let started = Instant::now();
    for _ in 0..n {
        f(&mut writer).expect("write benchmark frame");
    }
    let elapsed = started.elapsed();
    drop(writer);
    reader.join().expect("reader thread").expect("drain reader");

    BenchStats {
        mean_ns: elapsed.as_nanos() / n as u128,
    }
}

fn drain(mut reader: UnixStream) -> io::Result<()> {
    let mut buf = [0u8; 64 * 1024];
    while reader.read(&mut buf)? != 0 {}
    Ok(())
}

struct BenchStats {
    mean_ns: u128,
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
