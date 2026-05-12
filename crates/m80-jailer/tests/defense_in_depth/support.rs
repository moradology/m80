#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use m80_cgroup::{Limits, Subtree};
use m80_jailer::{
    BindMode, Binding, JailedFirecracker, JailerConfig, JailerSocket, MaterializedJail, Plan,
    ResourceLimits,
};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{chown, Gid, Pid, Uid};

const ATTACK_CONFIG_DEST: &str = "m80-attack-runner.conf";

pub(crate) struct AttackRun {
    pub(crate) exit_code: Option<i32>,
    pub(crate) cgroup_path: Option<PathBuf>,
    pub(crate) cgroup_contained_pid: bool,
}

pub(crate) struct LiveAttack {
    pub(crate) jail: MaterializedJail,
    pub(crate) jailed: JailedFirecracker,
    cgroup: Option<Subtree>,
}

pub(crate) struct AttackConfig<'a> {
    pub(crate) peer_sentinel: &'a str,
    pub(crate) peer_run_dir: &'a str,
    pub(crate) peer_network_state: &'a str,
    pub(crate) peer_pid: u32,
}

pub(crate) struct TenantSpec {
    pub(crate) run_dir: PathBuf,
    pub(crate) config_path: PathBuf,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl TenantSpec {
    pub(crate) fn new(root: &Path, name: &str, uid: u32, gid: u32) -> Self {
        let run_dir = root.join(name);
        Self {
            config_path: root.join(format!("{name}.conf")),
            run_dir,
            uid,
            gid,
        }
    }
}

pub(crate) fn assert_attack_blocked(name: &str) {
    let result = run_attack_in_jailer(name).expect("run attack");
    assert_ne!(
        result.exit_code,
        Some(0),
        "{name} escaped the jail; exit_code={:?}",
        result.exit_code
    );
}

pub(crate) fn assert_resource_attack_blocked(name: &str) {
    let result = run_attack_in_jailer_with_cgroup(name, resource_attack_limits())
        .expect("run cgroup-enrolled resource attack");
    assert_ne!(
        result.exit_code,
        Some(0),
        "{name} exhausted resources without hitting a configured limit"
    );
    assert!(
        result.cgroup_contained_pid,
        "resource attack must run after cgroup enrollment"
    );
}

pub(crate) fn assert_file_size_attack_blocked() {
    let result = run_attack_in_jailer_with_cgroup_and_resource_limits(
        "create_large_tmp_file",
        resource_attack_limits(),
        ResourceLimits {
            fsize: Some(1024 * 1024),
            ..ResourceLimits::default()
        },
    )
    .expect("run file-size resource attack");

    assert_ne!(
        result.exit_code,
        Some(0),
        "create_large_tmp_file wrote past the configured file-size limit"
    );
    assert!(
        result.cgroup_contained_pid,
        "resource attack must run after cgroup enrollment"
    );
}

pub(crate) fn assert_cross_tenant_attack_blocked(name: &str, extra_bindings: Vec<Binding>) {
    let temp = tempfile::tempdir().expect("tempdir");
    let tenant_a = TenantSpec::new(temp.path(), "tenant-a", 3000, 3000);
    let tenant_b = TenantSpec::new(temp.path(), "tenant-b", 3001, 3001);
    let peer_private = prepare_peer_private(temp.path(), tenant_b.uid, tenant_b.gid)
        .expect("prepare peer private dir");

    write_attack_config(
        &tenant_b.config_path,
        AttackConfig {
            peer_sentinel: "/unused-peer-a/sentinel",
            peer_run_dir: "/unused-peer-a/run",
            peer_network_state: "/unused-peer-a/network-state.json",
            peer_pid: 1,
        },
    )
    .expect("write tenant-b config");
    let live_b = launch_attack_in_jailer(
        "sleep_briefly",
        &tenant_b.run_dir,
        tenant_b.uid,
        tenant_b.gid,
        vec![config_binding(tenant_b.config_path.clone())],
        None,
    )
    .expect("launch peer tenant");

    let peer_in_jail =
        PathBuf::from("peers").join(peer_private.file_name().expect("peer private dir has name"));
    let peer_in_jail_display = format!("/{}", peer_in_jail.display());
    write_attack_config(
        &tenant_a.config_path,
        AttackConfig {
            peer_sentinel: &format!("{peer_in_jail_display}/sentinel"),
            peer_run_dir: &peer_in_jail_display,
            peer_network_state: &format!("{peer_in_jail_display}/network-state.json"),
            peer_pid: live_b.jailed.firecracker_pid(),
        },
    )
    .expect("write tenant-a config");

    let mut attacker_bindings = vec![
        config_binding(tenant_a.config_path.clone()),
        ro_binding(temp.path().to_path_buf(), PathBuf::from("peers")),
    ];
    attacker_bindings.extend(extra_bindings);
    let live_a = launch_attack_in_jailer(
        name,
        &tenant_a.run_dir,
        tenant_a.uid,
        tenant_a.gid,
        attacker_bindings,
        None,
    )
    .expect("launch attacker tenant");

    let result_a = live_a.wait().expect("wait attacker tenant");
    assert_ne!(
        result_a.exit_code,
        Some(0),
        "{name} reached the peer tenant; exit_code={:?}",
        result_a.exit_code
    );
    assert!(
        proc_pid_exists(live_b.jailed.firecracker_pid()),
        "{name} must not kill the peer tenant process"
    );
    let result_b = live_b.wait().expect("wait peer tenant");
    assert_eq!(result_b.exit_code, Some(0));
}

pub(crate) fn resource_attack_limits() -> Limits {
    Limits {
        memory_max: Some(64 * 1024 * 1024),
        pids_max: Some(32),
        ..Limits::preset()
    }
}

pub(crate) fn run_attack_in_jailer(name: &str) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, Vec::new(), None, ResourceLimits::default())
}

pub(crate) fn run_attack_in_jailer_with_bindings(
    name: &str,
    bindings: Vec<Binding>,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, bindings, None, ResourceLimits::default())
}

pub(crate) fn run_attack_in_jailer_with_cgroup(
    name: &str,
    limits: Limits,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, Vec::new(), Some(limits), ResourceLimits::default())
}

pub(crate) fn run_attack_in_jailer_with_cgroup_and_resource_limits(
    name: &str,
    limits: Limits,
    resource_limits: ResourceLimits,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, Vec::new(), Some(limits), resource_limits)
}

fn run_attack_in_jailer_inner(
    name: &str,
    bindings: Vec<Binding>,
    limits: Option<Limits>,
    resource_limits: ResourceLimits,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let run_dir = temp.path().join(format!("attack-{name}"));
    let live = launch_attack_in_jailer_with_resource_limits(
        name,
        &run_dir,
        3000,
        3000,
        bindings,
        limits,
        resource_limits,
    )?;

    live.wait()
}

pub(crate) fn launch_attack_in_jailer(
    name: &str,
    run_dir: &Path,
    uid: u32,
    gid: u32,
    bindings: Vec<Binding>,
    limits: Option<Limits>,
) -> Result<LiveAttack, Box<dyn std::error::Error>> {
    launch_attack_in_jailer_with_resource_limits(
        name,
        run_dir,
        uid,
        gid,
        bindings,
        limits,
        ResourceLimits::default(),
    )
}

pub(crate) fn launch_attack_in_jailer_with_resource_limits(
    name: &str,
    run_dir: &Path,
    uid: u32,
    gid: u32,
    bindings: Vec<Binding>,
    limits: Option<Limits>,
    resource_limits: ResourceLimits,
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
        resource_limits,
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
    pub(crate) fn wait(self) -> Result<AttackRun, Box<dyn std::error::Error>> {
        let cgroup_path = read_cgroup_path_record(self.jail.run_dir())?;
        let jailed_pid = self.jailed.firecracker_pid().to_string();
        let cgroup_contained_pid = cgroup_path.as_ref().is_some_and(|path| {
            match std::fs::read_to_string(path.join("cgroup.procs")) {
                Ok(procs) => procs.lines().any(|pid| pid == jailed_pid),
                Err(_) => false,
            }
        });
        let status = waitpid(Pid::from_raw(self.jailed.firecracker_pid() as i32), None)?;
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

pub(crate) fn config_binding(source: PathBuf) -> Binding {
    ro_binding(source, PathBuf::from(ATTACK_CONFIG_DEST))
}

pub(crate) fn ro_binding(source: PathBuf, dest: PathBuf) -> Binding {
    Binding {
        source,
        dest,
        mode: BindMode::Ro,
    }
}

pub(crate) fn create_inside_jail(dest: PathBuf) -> Binding {
    Binding {
        source: PathBuf::new(),
        dest,
        mode: BindMode::CreateInsideJail,
    }
}

pub(crate) fn write_attack_config(
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

pub(crate) fn proc_pid_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn prepare_peer_private(
    root: &Path,
    uid: u32,
    gid: u32,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = root.join("peer-private");
    std::fs::create_dir(&dir)?;
    std::fs::write(dir.join("sentinel"), b"peer tenant sentinel\n")?;
    std::fs::write(
        dir.join("network-state.json"),
        b"{\"tenant\":\"peer\",\"network\":\"private\"}\n",
    )?;
    set_mode(&dir, 0o700)?;
    set_mode(&dir.join("sentinel"), 0o600)?;
    set_mode(&dir.join("network-state.json"), 0o600)?;
    chown_tree(&dir, uid, gid)?;
    Ok(dir)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

fn chown_tree(path: &Path, uid: u32, gid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let uid = Uid::from_raw(uid);
    let gid = Gid::from_raw(gid);
    chown(path, Some(uid), Some(gid))?;
    for entry in std::fs::read_dir(path)? {
        chown(&entry?.path(), Some(uid), Some(gid))?;
    }
    Ok(())
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
