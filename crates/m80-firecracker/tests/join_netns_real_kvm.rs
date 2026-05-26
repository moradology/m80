//! Real-KVM coverage for functional `NetworkPolicy::JoinNetns` guest traffic.

mod common;

use std::ffi::OsStr;
use std::io::Cursor;
use std::net::Ipv4Addr;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

#[test]
#[ignore = "requires-kvm requires-root requires-network-namespace requires-artifacts"]
fn join_netns_routes_guest_traffic_through_caller_namespace() {
    let topology = JoinNetnsTopology::create();
    assert_other_namespace_cannot_reach_peer(&topology);
    let peer_marker = format!("m80-join-netns-{}", unique_suffix());
    let _peer_server = PeerTcpServer::start(&topology, &peer_marker);

    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig::builder(discovery)
        .max_concurrent_vms(1)
        .run_root(run_root)
        .jail_uid(3000)
        .jail_gid(3000)
        .cgroup_mode(CgroupMode::Disabled)
        .build();
    let backend = std::sync::Arc::new(Backend::new(config).expect("Backend::new"));
    let sandbox = backend
        .admit(SandboxConfig {
            vm_id: Some(unique_name("join-route")),
            workspace: None,
            network: NetworkPolicy::JoinNetns {
                spec: m80_firecracker::NetnsSpec {
                    netns_path: topology.join.path.clone(),
                    tap_name: topology.tap_name.clone(),
                    guest_mac: m80_firecracker::MacAddr::parse("02:00:00:00:80:02")
                        .expect("valid guest MAC"),
                    guest_ipv4: topology.guest_ipv4,
                    gateway_ipv4: topology.peer_ipv4,
                    dns_resolvers: vec![topology.peer_ipv4],
                },
            },
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            huge_pages_2m: false,
            cpuset_cpus: None,
            cpu_template: None,
            fc_log_level: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
            overlay_clone_mode: Default::default(),
            idle_timeout: None,
            max_lifetime: None,
            daemonize: false,
            request_id: None,
            pmem_layers: Vec::new(),
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch join-netns");
    let run_dir = running.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let tcp_client = compile_tcp_client_binary();
    running
        .upload_file_chunked(
            "/tmp/m80-tcp-connect",
            Some(0o755),
            Cursor::new(tcp_client),
            1024 * 1024,
        )
        .expect("upload tcp client");

    let response = running
        .exec(tcp_connect_request(
            topology.peer_ipv4,
            topology.peer_port,
            &peer_marker,
        ))
        .expect("exec guest TCP probe through join netns");

    let stopped = running.stop().expect("stop");
    if response.status == ExecStatus::Completed
        && response.exit_code == Some(0)
        && String::from_utf8_lossy(&response.stdout).contains(&peer_marker)
    {
        stopped.delete().expect("delete");
        return;
    }

    let preserved = stopped.preserve_for_triage().expect("preserve run dir");
    panic!(
        "JoinNetns guest TCP probe failed: status={:?}; exit={:?}; stdout={:?}; stderr={:?}; preserved run dir={}",
        response.status,
        response.exit_code,
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr),
        preserved.display()
    );
}

fn tcp_connect_request(peer_ipv4: Ipv4Addr, peer_port: u16, marker: &str) -> ExecRequest {
    ExecRequest {
        program: "/tmp/m80-tcp-connect".into(),
        args: vec![peer_ipv4.to_string(), peer_port.to_string(), marker.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(7_000),
        streaming: false,
    }
}

fn compile_tcp_client_binary() -> Vec<u8> {
    let build_dir = tempfile::tempdir().expect("tempdir for tcp client build");
    let source = build_dir.path().join("m80-tcp-connect.c");
    let output = build_dir.path().join("m80-tcp-connect");
    std::fs::write(
        &source,
        r#"
#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s <ipv4> <port> <marker>\n", argv[0]);
        return 2;
    }

    char *end = NULL;
    long port = strtol(argv[2], &end, 10);
    if (end == argv[2] || *end != '\0' || port <= 0 || port > 65535) {
        fprintf(stderr, "invalid port\n");
        return 2;
    }

    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) {
        fprintf(stderr, "socket: %s\n", strerror(errno));
        return 3;
    }

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    if (inet_pton(AF_INET, argv[1], &addr.sin_addr) != 1) {
        fprintf(stderr, "invalid address\n");
        close(fd);
        return 2;
    }

    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        fprintf(stderr, "connect: %s\n", strerror(errno));
        close(fd);
        return 4;
    }

    char buf[256];
    ssize_t n = read(fd, buf, sizeof(buf) - 1);
    if (n < 0) {
        fprintf(stderr, "read: %s\n", strerror(errno));
        close(fd);
        return 5;
    }
    buf[n] = '\0';
    close(fd);

    fputs(buf, stdout);
    if (strstr(buf, argv[3]) == NULL) {
        fprintf(stderr, "missing marker\n");
        return 6;
    }
    return 0;
}
"#,
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", source.display()));

    let status = Command::new("cc")
        .arg("-std=c11")
        .arg("-O2")
        .arg("-static")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-o")
        .arg(&output)
        .arg(&source)
        .status()
        .unwrap_or_else(|e| panic!("spawn cc for {}: {e}", source.display()));
    assert!(
        status.success(),
        "compile {} with cc -static failed: {status}",
        source.display()
    );

    std::fs::read(&output).unwrap_or_else(|e| panic!("read {}: {e}", output.display()))
}

struct JoinNetnsTopology {
    join: NetnsGuard,
    peer: NetnsGuard,
    other: NetnsGuard,
    tap_name: String,
    peer_ipv4: Ipv4Addr,
    peer_port: u16,
    guest_ipv4: ipnet::Ipv4Net,
}

impl JoinNetnsTopology {
    fn create() -> Self {
        let suffix = unique_suffix();
        let join = NetnsGuard::create(&format!("m80j{suffix}"));
        let peer = NetnsGuard::create(&format!("m80p{suffix}"));
        let other = NetnsGuard::create(&format!("m80o{suffix}"));

        let bridge = format!("br{suffix}");
        let tap_name = format!("tap{suffix}");
        let join_veth = format!("vj{suffix}");
        let peer_veth = format!("vp{suffix}");
        let subnet_octet = 10 + (u8::from_str_radix(&suffix[0..2], 16).unwrap() % 200);
        let peer_ipv4 = Ipv4Addr::new(10, 203, subnet_octet, 1);
        let peer_port = 18_080;
        let guest_ipv4 = format!("10.203.{subnet_octet}.2/24").parse().unwrap();

        run(
            "ip",
            [
                "netns", "exec", &join.name, "ip", "link", "add", &bridge, "type", "bridge",
            ],
            "create join bridge",
        );
        run(
            "ip",
            [
                "netns", "exec", &join.name, "ip", "tuntap", "add", "dev", &tap_name, "mode", "tap",
            ],
            "create join tap",
        );
        run(
            "ip",
            [
                "link", "add", &join_veth, "type", "veth", "peer", "name", &peer_veth,
            ],
            "create veth pair",
        );
        run(
            "ip",
            ["link", "set", &join_veth, "netns", &join.name],
            "move join veth",
        );
        run(
            "ip",
            ["link", "set", &peer_veth, "netns", &peer.name],
            "move peer veth",
        );
        for link in [&tap_name, &join_veth] {
            run(
                "ip",
                [
                    "netns", "exec", &join.name, "ip", "link", "set", link, "master", &bridge,
                ],
                "attach link to bridge",
            );
        }
        for link in [&bridge, &tap_name, &join_veth] {
            run(
                "ip",
                ["netns", "exec", &join.name, "ip", "link", "set", link, "up"],
                "bring join link up",
            );
        }
        run(
            "ip",
            [
                "netns",
                "exec",
                &peer.name,
                "ip",
                "addr",
                "add",
                &format!("{peer_ipv4}/24"),
                "dev",
                &peer_veth,
            ],
            "assign peer address",
        );
        run(
            "ip",
            [
                "netns", "exec", &peer.name, "ip", "link", "set", &peer_veth, "up",
            ],
            "bring peer link up",
        );

        Self {
            join,
            peer,
            other,
            tap_name,
            peer_ipv4,
            peer_port,
            guest_ipv4,
        }
    }
}

struct PeerTcpServer {
    child: Child,
    _ready_dir: tempfile::TempDir,
}

impl PeerTcpServer {
    fn start(topology: &JoinNetnsTopology, marker: &str) -> Self {
        let ready_dir = tempfile::tempdir().expect("tcp server ready tempdir");
        let ready_path = ready_dir.path().join("ready");
        let script = r#"
import pathlib
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
marker = sys.argv[3].encode()
ready = pathlib.Path(sys.argv[4])

server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind((host, port))
server.listen(1)
ready.write_text("ready\n")
conn, _addr = server.accept()
with conn:
    conn.sendall(marker)
"#;
        let child = Command::new("ip")
            .args([
                "netns",
                "exec",
                &topology.peer.name,
                "python3",
                "-c",
                script,
            ])
            .arg(topology.peer_ipv4.to_string())
            .arg(topology.peer_port.to_string())
            .arg(marker)
            .arg(&ready_path)
            .spawn()
            .expect("start peer TCP server");

        wait_for_ready_file(&ready_path);
        Self {
            child,
            _ready_dir: ready_dir,
        }
    }
}

impl Drop for PeerTcpServer {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_ready_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.is_file() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("peer TCP server did not report ready at {}", path.display());
}

fn assert_other_namespace_cannot_reach_peer(topology: &JoinNetnsTopology) {
    let output = Command::new("ip")
        .args([
            "netns",
            "exec",
            &topology.other.name,
            "ping",
            "-c",
            "1",
            "-W",
            "1",
        ])
        .arg(topology.peer_ipv4.to_string())
        .output()
        .expect("run isolated netns ping");
    assert!(
        !output.status.success(),
        "isolated namespace unexpectedly reached {}; stdout={:?}; stderr={:?}",
        topology.peer_ipv4,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct NetnsGuard {
    path: std::path::PathBuf,
    name: String,
}

impl NetnsGuard {
    fn create(name: &str) -> Self {
        run("ip", ["netns", "add", name], "create netns");
        Self {
            path: std::path::PathBuf::from(format!("/var/run/netns/{name}")),
            name: name.to_owned(),
        }
    }
}

impl Drop for NetnsGuard {
    fn drop(&mut self) {
        let _ = Command::new("ip")
            .args(["netns", "del", &self.name])
            .status();
    }
}

fn run<I, S>(program: &str, args: I, context: &str)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("{context}: spawn {program}: {err}"));
    assert!(
        output.status.success(),
        "{context}: {program} failed status={}; stdout={:?}; stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}-{}", unique_suffix())
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{:06x}", nanos % 0x00ff_ffff)
}
