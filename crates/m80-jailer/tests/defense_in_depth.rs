use std::path::{Path, PathBuf};

use m80_cgroup::{Limits, Subtree};
use m80_jailer::{
    BindMode, Binding, JailedFirecracker, JailerConfig, JailerSocket, MaterializedJail, Plan,
    ResourceLimits,
};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;

const ATTACK_CONFIG_DEST: &str = "m80-attack-runner.conf";

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_echo_zero_negative_control_reports_breach() {
    let result = run_attack_in_jailer("echo_zero").expect("run attack");

    assert_eq!(
        result.exit_code,
        Some(0),
        "echo_zero must prove the harness sees a successful attack as a breach"
    );
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_unknown_attack_reports_blocked() {
    let result = run_attack_in_jailer("missing_attack").expect("run attack");

    assert_ne!(
        result.exit_code,
        Some(0),
        "unknown attack must exercise the harness blocked/nonzero path"
    );
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_peer_config_transport_survives_env_clear() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_path = temp.path().join("attack-runner.conf");
    write_attack_config(
        &config_path,
        AttackConfig {
            peer_sentinel: "/peer/sentinel",
            peer_run_dir: "/peer/run",
            peer_network_state: "/peer/network-state.json",
            peer_pid: 42,
        },
    )
    .expect("write attack config");

    let result = run_attack_in_jailer_with_bindings(
        "require_peer_config",
        vec![config_binding(config_path)],
    )
    .expect("run attack with config");

    assert_eq!(
        result.exit_code,
        Some(0),
        "require_peer_config proves fixed-file config survived env_clear"
    );
}

#[test]
#[ignore = "requires root, writable cgroup v2, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn attack_runner_can_be_enrolled_in_m80_cgroup_limits() {
    let result = run_attack_in_jailer_with_cgroup("sleep_briefly", Limits::m80_default())
        .expect("run cgroup-enrolled attack");

    assert_eq!(
        result.exit_code,
        Some(0),
        "sleep_briefly must preserve the live harness control"
    );
    assert!(
        result.cgroup_path.is_some(),
        "cgroup enrollment must record cgroup-path.txt"
    );
    assert!(
        result.cgroup_contained_pid,
        "cgroup.procs must contain the jailed attack-runner pid before wait"
    );
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn two_tenant_attack_runner_fixture_materializes_distinct_live_jails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tenant_a = TenantSpec::new(temp.path(), "tenant-a", 3000, 3000);
    let tenant_b = TenantSpec::new(temp.path(), "tenant-b", 3001, 3001);

    write_attack_config(
        &tenant_a.config_path,
        AttackConfig {
            peer_sentinel: "/peer-b/sentinel",
            peer_run_dir: "/peer-b/run",
            peer_network_state: "/peer-b/network-state.json",
            peer_pid: 1,
        },
    )
    .expect("write tenant-a config");
    write_attack_config(
        &tenant_b.config_path,
        AttackConfig {
            peer_sentinel: "/peer-a/sentinel",
            peer_run_dir: "/peer-a/run",
            peer_network_state: "/peer-a/network-state.json",
            peer_pid: 1,
        },
    )
    .expect("write tenant-b config");

    let live_a = launch_attack_in_jailer(
        "sleep_briefly",
        &tenant_a.run_dir,
        tenant_a.uid,
        tenant_a.gid,
        vec![config_binding(tenant_a.config_path.clone())],
        None,
    )
    .expect("launch tenant-a attack");
    let live_b = launch_attack_in_jailer(
        "sleep_briefly",
        &tenant_b.run_dir,
        tenant_b.uid,
        tenant_b.gid,
        vec![config_binding(tenant_b.config_path.clone())],
        None,
    )
    .expect("launch tenant-b attack");

    assert_ne!(tenant_a.uid, tenant_b.uid, "tenants must use distinct uids");
    assert_ne!(tenant_a.gid, tenant_b.gid, "tenants must use distinct gids");
    assert_ne!(
        live_a.jailed.firecracker_pid, live_b.jailed.firecracker_pid,
        "two tenants must be distinct host processes"
    );
    assert!(
        proc_pid_exists(live_a.jailed.firecracker_pid)
            && proc_pid_exists(live_b.jailed.firecracker_pid),
        "both attack-runner processes must be live before cross-tenant tests run"
    );

    let result_a = live_a.wait().expect("wait tenant-a attack");
    let result_b = live_b.wait().expect("wait tenant-b attack");
    assert_eq!(result_a.exit_code, Some(0));
    assert_eq!(result_b.exit_code, Some(0));
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_chroot_escape_via_dotdot() {
    assert_attack_blocked("chroot_escape_via_dotdot");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_chroot_escape_via_openat_style_path() {
    assert_attack_blocked("chroot_escape_via_openat_style_path");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_chroot_escape_via_proc_self_root() {
    assert_attack_blocked("chroot_escape_via_proc_self_root");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_host_sentinel() {
    assert_attack_blocked("read_host_sentinel");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_write_host_sentinel() {
    assert_attack_blocked("write_host_sentinel");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_write_to_lower_layer() {
    assert_attack_blocked("write_to_lower_layer");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_observe_host_pid_status() {
    assert_attack_blocked("observe_host_pid_status");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_signal_host_pid_probe() {
    assert_attack_blocked("signal_host_pid_probe");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_host_pid_cmdline() {
    assert_attack_blocked("read_host_pid_cmdline");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_enumerate_host_processes() {
    assert_attack_blocked("enumerate_host_processes");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_read_host_proc_mountinfo() {
    assert_attack_blocked("read_host_proc_mountinfo");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_become_uid_zero() {
    assert_attack_blocked("become_uid_zero");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_become_gid_zero() {
    assert_attack_blocked("become_gid_zero");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_retain_effective_capabilities() {
    assert_attack_blocked("retain_effective_capabilities");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_unshare_mount_namespace() {
    assert_attack_blocked("unshare_mount_namespace");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mount_tmpfs() {
    assert_attack_blocked("mount_tmpfs");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_change_hostname() {
    assert_attack_blocked("change_hostname");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_open_raw_socket() {
    assert_attack_blocked("open_raw_socket");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_open_raw_packet_socket() {
    assert_attack_blocked("raw_packet_inject");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mutate_links_over_netlink() {
    assert_attack_blocked("send_arbitrary_netlink");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_bind_host_only_address() {
    assert_attack_blocked("bind_on_host_interface");
}

#[test]
#[ignore = "requires root, official Firecracker jailer, m80-jailer-harden, and musl attack-runner"]
fn jailed_attacker_cannot_mutate_routes_over_netlink() {
    assert_attack_blocked("privileged_route_mutation");
}

struct AttackRun {
    exit_code: Option<i32>,
    cgroup_path: Option<PathBuf>,
    cgroup_contained_pid: bool,
}

struct LiveAttack {
    jail: MaterializedJail,
    jailed: JailedFirecracker,
    cgroup: Option<Subtree>,
}

struct AttackConfig<'a> {
    peer_sentinel: &'a str,
    peer_run_dir: &'a str,
    peer_network_state: &'a str,
    peer_pid: u32,
}

struct TenantSpec {
    run_dir: PathBuf,
    config_path: PathBuf,
    uid: u32,
    gid: u32,
}

impl TenantSpec {
    fn new(root: &Path, name: &str, uid: u32, gid: u32) -> Self {
        let run_dir = root.join(name);
        Self {
            config_path: root.join(format!("{name}.conf")),
            run_dir,
            uid,
            gid,
        }
    }
}

fn assert_attack_blocked(name: &str) {
    let result = run_attack_in_jailer(name).expect("run attack");
    assert_ne!(
        result.exit_code,
        Some(0),
        "{name} escaped the jail; exit_code={:?}",
        result.exit_code
    );
}

fn run_attack_in_jailer(name: &str) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, Vec::new(), None)
}

fn run_attack_in_jailer_with_bindings(
    name: &str,
    bindings: Vec<Binding>,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, bindings, None)
}

fn run_attack_in_jailer_with_cgroup(
    name: &str,
    limits: Limits,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, Vec::new(), Some(limits))
}

fn run_attack_in_jailer_inner(
    name: &str,
    bindings: Vec<Binding>,
    limits: Option<Limits>,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let run_dir = temp.path().join(format!("attack-{name}"));
    let live = launch_attack_in_jailer(name, &run_dir, 3000, 3000, bindings, limits)?;

    live.wait()
}

fn launch_attack_in_jailer(
    name: &str,
    run_dir: &Path,
    uid: u32,
    gid: u32,
    bindings: Vec<Binding>,
    limits: Option<Limits>,
) -> Result<LiveAttack, Box<dyn std::error::Error>> {
    std::fs::create_dir(run_dir)?;
    let stdio_log = run_dir.join("attack-runner.log");
    let config = JailerConfig {
        jailer_bin: binary_from_env("M80_JAILER_BIN", "/usr/bin/jailer")?,
        jailer_harden_bin: Some(binary_from_env(
            "M80_JAILER_HARDEN_BIN",
            "/usr/bin/m80-jailer-harden",
        )?),
        firecracker_bin: attack_runner_bin()?,
        run_dir: run_dir.to_path_buf(),
        uid,
        gid,
        bindings,
        sockets: vec![JailerSocket::Firecracker],
        resource_limits: ResourceLimits::default(),
        new_pid_ns: false,
        daemonize: false,
        new_cgroup_ns: false,
        netns_path: None,
        stdio_log: Some(stdio_log),
    };
    let jail = Plan::compute(&config)?.materialize()?;
    let jailed = jail.launch(Path::new(name))?;
    let cgroup = if let Some(limits) = limits {
        let vm_id = format!("attack-{name}-{}", std::process::id());
        Some(Subtree::create(&vm_id, &jail, &jailed, &limits)?)
    } else {
        None
    };
    Ok(LiveAttack {
        jail,
        jailed,
        cgroup,
    })
}

impl LiveAttack {
    fn wait(self) -> Result<AttackRun, Box<dyn std::error::Error>> {
        let cgroup_path = read_cgroup_path_record(&self.jail.plan.config.run_dir)?;
        let jailed_pid = self.jailed.firecracker_pid.to_string();
        let cgroup_contained_pid = cgroup_path.as_ref().is_some_and(|path| {
            match std::fs::read_to_string(path.join("cgroup.procs")) {
                Ok(procs) => procs.lines().any(|pid| pid == jailed_pid),
                Err(_) => false,
            }
        });
        let status = waitpid(Pid::from_raw(self.jailed.firecracker_pid as i32), None)?;
        drop(self.cgroup);
        Ok(AttackRun {
            exit_code: match status {
                WaitStatus::Exited(_, code) => Some(code),
                WaitStatus::Signaled(_, signal, _) => Some(128 + signal as i32),
                other => panic!("unexpected attack-runner wait status: {other:?}"),
            },
            cgroup_path,
            cgroup_contained_pid,
        })
    }
}

fn read_cgroup_path_record(run_dir: &Path) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let path = run_dir.join("cgroup-path.txt");
    if !path.exists() {
        return Ok(None);
    }

    Ok(Some(PathBuf::from(std::fs::read_to_string(path)?.trim())))
}

fn config_binding(source: PathBuf) -> Binding {
    Binding {
        source,
        dest: PathBuf::from(ATTACK_CONFIG_DEST),
        mode: BindMode::Ro,
    }
}

fn write_attack_config(
    path: &Path,
    config: AttackConfig<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(
        path,
        format!(
            "peer_sentinel={}\npeer_run_dir={}\npeer_network_state={}\npeer_pid={}\n",
            config.peer_sentinel, config.peer_run_dir, config.peer_network_state, config.peer_pid
        ),
    )?;
    Ok(())
}

fn proc_pid_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn binary_from_env(env: &str, default: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let raw = std::env::var(env).unwrap_or_else(|_| default.to_owned());
    Ok(std::fs::canonicalize(raw)?)
}

fn attack_runner_bin() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("M80_ATTACK_RUNNER_BIN") {
        return Ok(std::fs::canonicalize(path)?);
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate lives under workspace/crates");
    let path = workspace
        .join("target")
        .join("x86_64-unknown-linux-musl")
        .join("debug")
        .join("m80-attack-runner");
    Ok(std::fs::canonicalize(path)?)
}
