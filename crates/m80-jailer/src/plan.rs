//! `Plan::compute` (pure) and `Plan::materialize` (mounts + persists state).

use std::io;

use crate::error::JailerError;
use crate::materialized::MaterializedJail;
use crate::types::{
    jail_root_path, BindMode, JailerConfig, JailerState, Plan, PlanStep, JAILER_PLAN_FILE,
    JAILER_STATE_FILE,
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

        // Reject absolute `binding.dest`: Path::join silently drops the
        // jail_root prefix when given an absolute path, which would let a
        // caller bind-mount outside the jail (e.g., dest = "/etc").
        for binding in &config.bindings {
            if binding.dest.is_absolute() {
                return Err(JailerError::BindFailed {
                    src: binding.source.clone(),
                    dest: binding.dest.clone(),
                });
            }
        }

        let jail_root = jail_root_path(&config.run_dir, &config.firecracker_bin);
        let mut steps = Vec::new();

        // Step 1: create the jail root.
        steps.push(PlanStep::CreateDir {
            path: jail_root.clone(),
            mode: 0o755,
        });

        // Step 2: CreateInsideJail entries (create dir, no source).
        for binding in &config.bindings {
            if binding.mode == BindMode::CreateInsideJail {
                steps.push(PlanStep::CreateDir {
                    path: jail_root.join(&binding.dest),
                    mode: 0o755,
                });
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
        use nix::unistd::mkdir;

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
                PlanStep::CreateDir { path, .. } => {
                    mkdir(path, Mode::from_bits_truncate(0o755)).map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    fchmodat(
                        None,
                        path,
                        Mode::from_bits_truncate(0o755),
                        FchmodatFlags::FollowSymlink,
                    )
                    .map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    materialized.created_dirs.push(path.clone());
                }
                PlanStep::Bind { source, dest, mode } => {
                    if !dest.is_dir() && !source.is_dir() {
                        // dest dir was already created or is the jail root
                        std::fs::write(dest, b"").map_err(|source| JailerError::Io {
                            path: dest.clone(),
                            source,
                        })?;
                        materialized.placeholder_files.push(dest.clone());
                    }

                    mount(
                        Some(source.as_path()),
                        dest.as_path(),
                        None::<&str>,
                        MsFlags::MS_BIND,
                        None::<&str>,
                    )
                    .map_err(|_e| JailerError::BindFailed {
                        src: source.clone(),
                        dest: dest.clone(),
                    })?;
                    materialized.bind_mounts.push(dest.clone());

                    if *mode == BindMode::Ro {
                        mount(
                            None::<&str>,
                            dest.as_path(),
                            None::<&str>,
                            MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY,
                            None::<&str>,
                        )
                        .map_err(|_e| JailerError::BindFailed {
                            src: source.clone(),
                            dest: dest.clone(),
                        })?;
                    } else {
                        // Rw bind: chown the source so the jailed firecracker
                        // (running as `config.uid`) can open it for writing.
                        // Bind mounts share the inode with the source, so a
                        // chown of the source path is what the in-chroot
                        // firecracker actually sees.
                        use nix::unistd::{chown, Gid, Uid};
                        chown(
                            source.as_path(),
                            Some(Uid::from_raw(materialized.plan.config.uid)),
                            Some(Gid::from_raw(materialized.plan.config.gid)),
                        )
                        .map_err(|_e| JailerError::BindFailed {
                            src: source.clone(),
                            dest: dest.clone(),
                        })?;
                    }
                }
                PlanStep::Socket { .. } => {
                    // Firecracker creates the UDS itself; nothing to do here.
                }
            }
        }

        let plan_path = materialized.plan.config.run_dir.join(JAILER_PLAN_FILE);
        let plan_json = serde_json::to_vec_pretty(&materialized.plan).map_err(|e| JailerError::Io {
            path: plan_path.clone(),
            source: io::Error::new(io::ErrorKind::Other, e),
        })?;
        std::fs::write(&plan_path, &plan_json).map_err(|source| JailerError::Io {
            path: plan_path.clone(),
            source,
        })?;

        let state_path = materialized.plan.config.run_dir.join(JAILER_STATE_FILE);
        let state = JailerState {
            jailer_pid: None,
            firecracker_pid: None,
        };
        let state_json = serde_json::to_vec_pretty(&state).map_err(|e| JailerError::Io {
            path: state_path.clone(),
            source: io::Error::new(io::ErrorKind::Other, e),
        })?;
        std::fs::write(&state_path, &state_json).map_err(|source| JailerError::Io {
            path: state_path.clone(),
            source,
        })?;

        Ok(materialized)
    }
}
