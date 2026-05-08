use std::path::{Path, PathBuf};

use m80_cgroup::{Limits, Subtree};
use m80_jailer::{
    BindMode, Binding, JailedFirecracker, JailerConfig, JailerSocket, MaterializedJail, Plan,
    ResourceLimits,
};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;

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

pub(crate) fn run_attack_in_jailer(name: &str) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, Vec::new(), None)
}

pub(crate) fn run_attack_in_jailer_with_bindings(
    name: &str,
    bindings: Vec<Binding>,
) -> Result<AttackRun, Box<dyn std::error::Error>> {
    run_attack_in_jailer_inner(name, bindings, None)
}

pub(crate) fn run_attack_in_jailer_with_cgroup(
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

pub(crate) fn launch_attack_in_jailer(
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
    pub(crate) fn wait(self) -> Result<AttackRun, Box<dyn std::error::Error>> {
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
