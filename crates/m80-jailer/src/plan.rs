//! `Plan::compute` (pure) and `Plan::materialize` (mounts + persists state).

use std::collections::BTreeSet;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use crate::error::JailerError;
use crate::materialized::MaterializedJail;
use crate::types::{
    check_plan_basenames, jail_root_path, BindMode, JailerConfig, JailerState, Plan, PlanStep,
    JAILER_PLAN_FILE, JAILER_STATE_FILE,
};

impl Plan {
    /// Compute a plan from a config without touching the filesystem. Steps
    /// are emitted in a canonical order so the persisted plan replays
    /// deterministically; see the `// Step N:` markers in the body.
    pub fn compute(config: &JailerConfig) -> Result<Plan, JailerError> {
        if config.uid == 0 || config.gid == 0 {
            return Err(JailerError::UidGidInvalid {
                uid: config.uid,
                gid: config.gid,
            });
        }
        check_plan_basenames(&config.run_dir, &config.firecracker_bin)?;

        // Reject any destination that can escape the jail root or expose
        // host-kernel virtual filesystems inside the jail.
        for binding in &config.bindings {
            if invalid_dest(&binding.dest) || is_hidden_kernel_dest(&binding.dest) {
                return Err(JailerError::BindDestRejected {
                    src: binding.source.clone(),
                    dest: binding.dest.clone(),
                });
            }
        }

        let jail_root = jail_root_path(&config.run_dir, &config.firecracker_bin);
        let mut steps = Vec::new();
        let mut created_dirs = BTreeSet::new();

        // Step 1: create the jail root.
        push_create_dir(&mut steps, &mut created_dirs, jail_root.clone());

        // Step 2: CreateInsideJail entries and bind parent directories.
        for binding in &config.bindings {
            push_parent_dirs(&mut steps, &mut created_dirs, &jail_root, &binding.dest);
            if binding.mode == BindMode::CreateInsideJail {
                push_create_dir(&mut steps, &mut created_dirs, jail_root.join(&binding.dest));
            }
        }

        // Step 3: Ro/Rw bind mounts.
        for binding in &config.bindings {
            if binding.mode == BindMode::Ro || binding.mode == BindMode::Rw {
                steps.push(PlanStep::Bind {
                    source: binding.source.clone(),
                    dest: jail_root.join(&binding.dest),
                    mode: binding.mode,
                });
            }
        }

        // Step 4: socket reservations.
        for socket in &config.sockets {
            steps.push(PlanStep::Socket {
                path: jail_root.join(socket.jail_path()),
            });
        }

        Ok(Plan {
            schema_version: 1,
            config: config.clone(),
            steps,
        })
    }

    /// Materialize the plan: create dirs, perform binds, persist state.
    ///
    /// Requires `CAP_SYS_ADMIN` (or root). Persists `jailer-plan.json` and an
    /// initial `jailer-state.json` in the run-dir on success. `Drop` on the
    /// returned [`MaterializedJail`] tears down the chroot.
    pub fn materialize(self) -> Result<MaterializedJail, JailerError> {
        use nix::mount::{mount, MsFlags};
        use nix::sys::stat::{fchmodat, FchmodatFlags, Mode};
        use nix::unistd::{chown, mkdir, Gid, Uid};

        let jail_root = jail_root_path(&self.config.run_dir, &self.config.firecracker_bin);

        // Pre-create the two intermediate dirs jailer expects to exist
        // (`<run_dir>/<exec basename>/` and `<run_dir>/<exec basename>/<id>/`)
        // so the per-step `mkdir` for jail_root itself can succeed. We don't
        // track these for cleanup — Drop runs `remove_dir` on jail_root, which
        // reclaims the leaf; the empty parent dirs are removed by run-dir
        // teardown later.
        if let Some(parent) = jail_root.parent() {
            std::fs::create_dir_all(parent).map_err(|source| JailerError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        // Construct the guard up-front so that on any `?`-propagated error the
        // partially-applied state (created dirs + bind mounts so far) gets
        // unwound by `Drop` instead of leaking into the host's mount table.
        let mut materialized = MaterializedJail {
            jail_path: jail_root,
            bind_mounts: Vec::new(),
            created_dirs: Vec::new(),
            placeholder_files: Vec::new(),
            plan: self,
        };

        for step in &materialized.plan.steps {
            match step {
                PlanStep::CreateDir { path, mode } => {
                    mkdir(path, Mode::from_bits_truncate(*mode)).map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    fchmodat(
                        None,
                        path,
                        Mode::from_bits_truncate(*mode),
                        FchmodatFlags::NoFollowSymlink,
                    )
                    .map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    chown(
                        path,
                        Some(Uid::from_raw(materialized.plan.config.uid)),
                        Some(Gid::from_raw(materialized.plan.config.gid)),
                    )
                    .map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    materialized.created_dirs.push(path.clone());
                }
                PlanStep::Bind { source, dest, mode } => {
                    let canonical_source =
                        std::fs::canonicalize(source).map_err(|io_source| JailerError::Io {
                            path: source.clone(),
                            source: io_source,
                        })?;

                    if !dest.is_dir() && !canonical_source.is_dir() {
                        // dest dir was already created or is the jail root
                        write_file_no_follow(dest, b"")?;
                        materialized.placeholder_files.push(dest.clone());
                    }

                    mount(
                        Some(canonical_source.as_path()),
                        dest.as_path(),
                        None::<&str>,
                        MsFlags::MS_BIND | MsFlags::MS_REC,
                        None::<&str>,
                    )
                    .map_err(|e| JailerError::BindFailed {
                        src: source.clone(),
                        dest: dest.clone(),
                        source: e,
                    })?;
                    materialized.bind_mounts.push(dest.clone());

                    if *mode == BindMode::Ro {
                        mount(
                            None::<&str>,
                            dest.as_path(),
                            None::<&str>,
                            bind_remount_flags() | MsFlags::MS_RDONLY,
                            None::<&str>,
                        )
                        .map_err(|e| JailerError::BindFailed {
                            src: source.clone(),
                            dest: dest.clone(),
                            source: e,
                        })?;
                    } else {
                        // Rw bind: chown the source so the jailed firecracker
                        // (running as `config.uid`) can open it for writing.
                        // Bind mounts share the inode with the source, so a
                        // chown of the source path is what the in-chroot
                        // firecracker actually sees.
                        chown(
                            canonical_source.as_path(),
                            Some(Uid::from_raw(materialized.plan.config.uid)),
                            Some(Gid::from_raw(materialized.plan.config.gid)),
                        )
                        .map_err(|e| JailerError::BindFailed {
                            src: source.clone(),
                            dest: dest.clone(),
                            source: e,
                        })?;
                        mount(
                            None::<&str>,
                            dest.as_path(),
                            None::<&str>,
                            bind_remount_flags(),
                            None::<&str>,
                        )
                        .map_err(|e| JailerError::BindFailed {
                            src: source.clone(),
                            dest: dest.clone(),
                            source: e,
                        })?;
                    }
                }
                PlanStep::Socket { .. } => {
                    // Firecracker creates the UDS itself; nothing to do here.
                }
            }
        }

        let plan_path = materialized.plan.config.run_dir.join(JAILER_PLAN_FILE);
        let plan_json =
            serde_json::to_vec_pretty(&materialized.plan).map_err(|e| JailerError::Io {
                path: plan_path.clone(),
                source: io::Error::new(io::ErrorKind::Other, e),
            })?;
        write_file_no_follow(&plan_path, &plan_json)?;

        let state_path = materialized.plan.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            schema_version: 1,
            jailer_pid: None,
            firecracker_pid: None,
        };
        let state_json = serde_json::to_vec_pretty(&state).map_err(|e| JailerError::Io {
            path: state_path.clone(),
            source: io::Error::new(io::ErrorKind::Other, e),
        })?;
        write_file_no_follow(&state_path, &state_json)?;

        Ok(materialized)
    }
}

fn invalid_dest(path: &Path) -> bool {
    let mut saw_component = false;
    for component in path.components() {
        saw_component = true;
        if !matches!(component, Component::Normal(_)) {
            return true;
        }
    }
    !saw_component
}

fn push_parent_dirs(
    steps: &mut Vec<PlanStep>,
    created_dirs: &mut BTreeSet<PathBuf>,
    jail_root: &Path,
    dest: &Path,
) {
    if let Some(parent) = dest.parent() {
        let mut path = jail_root.to_path_buf();
        for component in parent.components() {
            path.push(component.as_os_str());
            push_create_dir(steps, created_dirs, path.clone());
        }
    }
}

fn push_create_dir(steps: &mut Vec<PlanStep>, created_dirs: &mut BTreeSet<PathBuf>, path: PathBuf) {
    if created_dirs.insert(path.clone()) {
        steps.push(PlanStep::CreateDir { path, mode: 0o700 });
    }
}

fn is_hidden_kernel_dest(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Normal(name)) if name == "dev" || name == "proc" || name == "sys"
    )
}

fn bind_remount_flags() -> nix::mount::MsFlags {
    // MS_BIND is REQUIRED alongside MS_REMOUNT when remounting a bind-
    // mount: the kernel uses (MS_BIND|MS_REMOUNT) to disambiguate which
    // mount to target when the path participates in multiple mounts.
    // Dropping MS_BIND causes EBUSY at the second mount() call on the
    // bind dest. The earlier audit (m80-l020n.9) was wrong to remove it.
    nix::mount::MsFlags::MS_BIND
        | nix::mount::MsFlags::MS_REMOUNT
        | nix::mount::MsFlags::MS_NODEV
        | nix::mount::MsFlags::MS_NOEXEC
        | nix::mount::MsFlags::MS_NOSUID
}

pub(crate) fn write_file_no_follow(path: &Path, bytes: &[u8]) -> Result<(), JailerError> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| JailerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| JailerError::Io {
        path: path.to_path_buf(),
        source,
    })
}
