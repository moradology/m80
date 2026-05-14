//! Real-KVM coverage for functional `NetworkPolicy::JoinNetns` guest traffic.

mod common;

use std::ffi::OsStr;
use std::net::Ipv4Addr;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use m80_firecracker::{Backend, BackendConfig, CgroupMode, NetworkPolicy, SandboxConfig};
use m80_proto::{ExecRequest, ExecStatus};

use common::RunDirDumpGuard;

#[test]
#[ignore = "requires root, iproute2 netns support, KVM host, and real Firecracker binary"]
fn join_netns_routes_guest_traffic_through_caller_namespace() {
    let topology = JoinNetnsTopology::create();
    assert_other_namespace_cannot_reach_peer(&topology);

    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let run_root = discovery.run_root.clone();
    let config = BackendConfig {
        discovery,
        max_concurrent_vms: 1,
        run_root,
        jail_uid: 3000,
        jail_gid: 3000,
        cgroup_mode: CgroupMode::Disabled,
    };
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
            cpuset_cpus: None,
            cpu_template: None,
            drive_cache_type: None,
            boot_args: None,
            overlay_size_bytes: 256 * 1024 * 1024,
            idle_timeout: None,
            daemonize: false,
            request_id: None,
            preallocated_drive_slots: 0,
            one_shot: false,
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch join-netns");
    let run_dir = running.run_dir().to_owned();
    let _dump_guard = RunDirDumpGuard::new(run_dir);

    let response = running
        .exec(shell_request(&format!(
            "/bin/busybox ping -c 1 -W 2 {}",
            topology.peer_ipv4
        )))
        .expect("exec guest ping through join netns");

    let stopped = running.stop().expect("stop");
    if response.status == ExecStatus::Completed && response.exit_code == Some(0) {
        stopped.delete().expect("delete");
        return;
    }

    let preserved = stopped.preserve_for_triage().expect("preserve run dir");
    panic!(
        "JoinNetns guest ping failed: status={:?}; exit={:?}; stdout={:?}; stderr={:?}; preserved run dir={}",
        response.status,
        response.exit_code,
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr),
        preserved.display()
    );
}

fn shell_request(script: &str) -> ExecRequest {
    ExecRequest {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: None,
        stdin: None,
        timeout_ms: Some(7_000),
        streaming: false,
    }
}

struct JoinNetnsTopology {
    join: NetnsGuard,
    _peer: NetnsGuard,
    other: NetnsGuard,
    tap_name: String,
    peer_ipv4: Ipv4Addr,
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
            _peer: peer,
            other,
            tap_name,
            peer_ipv4,
            guest_ipv4,
        }
    }
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
