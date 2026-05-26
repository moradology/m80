//! `Plan::compute` (pure) and `Plan::materialize` (mounts + persists state).

use std::collections::BTreeSet;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use m80_image_store::DEFAULT_STORE_ROOT;
use nix::sys::stat::umask;

use crate::error::JailerError;
use crate::materialized::MaterializedJail;
use crate::types::{
    check_plan_basenames, jail_root_path, BindMode, JailerConfig, JailerState, Plan, PlanStep,
    JAILER_PLAN_FILE, JAILER_STATE_FILE,
};

const JAIL_ROOT_MODE: u32 = 0o730;
const JAIL_INTERNAL_DIR_MODE: u32 = 0o700;

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

        // Reject any destination that can escape the jail root, expose
        // host-kernel virtual filesystems inside the jail, or collide with
        // another bind destination. CreateInsideJail may share a destination
        // with a later bind because that is the explicit "create mount point,
        // then bind into it" pattern.
        let mut bind_dests = BTreeSet::new();
        for binding in &config.bindings {
            if invalid_dest(&binding.dest) || is_hidden_kernel_dest(&binding.dest) {
                return Err(JailerError::BindDestRejected {
                    src: binding.source.clone(),
                    dest: binding.dest.clone(),
                });
            }
            if is_bind_mount_mode(binding.mode) && !bind_dests.insert(binding.dest.clone()) {
                return Err(JailerError::BindDestRejected {
                    src: binding.source.clone(),
                    dest: binding.dest.clone(),
                });
            }
            if binding.mode == BindMode::RoImageStore
                && !image_store_source_has_default_root(&binding.source)
            {
                return Err(JailerError::BindSourceRejected {
                    src: binding.source.clone(),
                    expected_root: PathBuf::from(DEFAULT_STORE_ROOT),
                });
            }
        }

        let jail_root = jail_root_path(&config.run_dir, &config.firecracker_bin);
        let mut steps = Vec::new();
        let mut created_dirs = BTreeSet::new();

        // Step 1: create the jail root. The official jailer runs as root
        // after m80-jailer-harden has removed CAP_DAC_OVERRIDE, so the root
        // itself must remain root-owned at materialization time. The jailed
        // gid still needs write+search permission after the jailer drops
        // privilege so Firecracker can create its API/vsock sockets.
        push_create_dir_with_mode(
            &mut steps,
            &mut created_dirs,
            jail_root.clone(),
            JAIL_ROOT_MODE,
        );

        // Step 2: CreateInsideJail entries and bind parent directories.
        for binding in &config.bindings {
            push_parent_dirs(&mut steps, &mut created_dirs, &jail_root, &binding.dest);
            if binding.mode == BindMode::CreateInsideJail {
                push_create_dir(&mut steps, &mut created_dirs, jail_root.join(&binding.dest));
            }
        }

        // Step 3: bind mounts.
        for binding in &config.bindings {
            if is_bind_mount_mode(binding.mode) {
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
        use nix::mount::mount;
        use nix::sys::stat::Mode;
        use nix::unistd::{chown, mkdir, Gid, Uid};

        let jail_root = jail_root_path(&self.config.run_dir, &self.config.firecracker_bin);
        let _umask_guard = UmaskGuard::zero();

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
        let vm_id = crate::materialized::vm_id_for_plan(&self);
        let mut materialized = MaterializedJail {
            vm_id,
            jail_path: jail_root,
            bind_mounts: Vec::new(),
            created_dirs: Vec::new(),
            placeholder_files: Vec::new(),
            plan: self,
        };

        let mut mount_propagation_private = false;
        for step in &materialized.plan.steps {
            match step {
                PlanStep::CreateDir { path, mode } => {
                    mkdir(path, Mode::from_bits_truncate(*mode)).map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    let uid = if path == &materialized.jail_path {
                        Uid::from_raw(0)
                    } else {
                        Uid::from_raw(materialized.plan.config.uid)
                    };
                    chown(
                        path,
                        Some(uid),
                        Some(Gid::from_raw(materialized.plan.config.gid)),
                    )
                    .map_err(|e| JailerError::Io {
                        path: path.clone(),
                        source: io::Error::from_raw_os_error(e as i32),
                    })?;
                    materialized.created_dirs.push(path.clone());
                }
                PlanStep::Bind { source, dest, mode } => {
                    if !mount_propagation_private {
                        make_mounts_private()?;
                        mount_propagation_private = true;
                    }

                    let mount_source = bind_mount_source_for_mode(source, *mode)?;
                    let source_meta =
                        std::fs::metadata(mount_source.as_path()).map_err(|io_source| {
                            JailerError::Io {
                                path: source.clone(),
                                source: io_source,
                            }
                        })?;
                    let source_is_dir = source_meta.is_dir();
                    let dest_is_dir = dest.is_dir();

                    if !dest_is_dir && !source_is_dir {
                        // dest dir was already created or is the jail root
                        write_file_no_follow(dest, b"")?;
                        materialized.placeholder_files.push(dest.clone());
                    }

                    mount(
                        Some(mount_source.as_path()),
                        dest.as_path(),
                        None::<&str>,
                        bind_mount_flags(source_is_dir, dest_is_dir),
                        None::<&str>,
                    )
                    .map_err(|e| JailerError::BindFailed {
                        src: source.clone(),
                        dest: dest.clone(),
                        source: e,
                    })?;
                    materialized.bind_mounts.push(dest.clone());

                    if is_read_only_bind_mode(*mode) {
                        mount(
                            None::<&str>,
                            dest.as_path(),
                            None::<&str>,
                            bind_remount_flags_for_mode(*mode),
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
                            mount_source.as_path(),
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
        let plan_json = serde_json::to_vec(&materialized.plan).map_err(|e| JailerError::Io {
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
        let state_json = serde_json::to_vec(&state).map_err(|e| JailerError::Io {
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
    push_create_dir_with_mode(steps, created_dirs, path, JAIL_INTERNAL_DIR_MODE);
}

fn push_create_dir_with_mode(
    steps: &mut Vec<PlanStep>,
    created_dirs: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    mode: u32,
) {
    if created_dirs.insert(path.clone()) {
        steps.push(PlanStep::CreateDir { path, mode });
    }
}

fn is_hidden_kernel_dest(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Normal(name)) if name == "dev" || name == "proc" || name == "sys"
    )
}

fn is_bind_mount_mode(mode: BindMode) -> bool {
    matches!(mode, BindMode::Ro | BindMode::RoImageStore | BindMode::Rw)
}

fn is_read_only_bind_mode(mode: BindMode) -> bool {
    matches!(mode, BindMode::Ro | BindMode::RoImageStore)
}

fn image_store_source_has_default_root(source: &Path) -> bool {
    source.is_absolute() && source.starts_with(Path::new(DEFAULT_STORE_ROOT))
}

fn bind_remount_flags() -> nix::mount::MsFlags {
    // MS_BIND is REQUIRED alongside MS_REMOUNT when remounting a bind-
    // mount: the kernel uses (MS_BIND|MS_REMOUNT) to disambiguate which
    // mount to target when the path participates in multiple mounts.
    // Dropping MS_BIND causes EBUSY at the second mount() call on the
    // bind dest. The earlier audit (m80-l020n.9) was wrong to remove it.
    // Pinned by plan::tests::bind_remount_flags_is_exactly_expected_bitset.
    nix::mount::MsFlags::MS_BIND
        | nix::mount::MsFlags::MS_REMOUNT
        | nix::mount::MsFlags::MS_NODEV
        | nix::mount::MsFlags::MS_NOEXEC
        | nix::mount::MsFlags::MS_NOSUID
}

fn bind_remount_flags_for_mode(mode: BindMode) -> nix::mount::MsFlags {
    let flags = bind_remount_flags();
    if is_read_only_bind_mode(mode) {
        flags | nix::mount::MsFlags::MS_RDONLY
    } else {
        flags
    }
}

fn bind_mount_source(source: &Path) -> Result<PathBuf, io::Error> {
    if is_proc_fd_path(source) {
        return Ok(source.to_path_buf());
    }
    std::fs::canonicalize(source)
}

fn bind_mount_source_for_mode(source: &Path, mode: BindMode) -> Result<PathBuf, JailerError> {
    let mount_source = bind_mount_source(source).map_err(|io_source| JailerError::Io {
        path: source.to_path_buf(),
        source: io_source,
    })?;
    if mode == BindMode::RoImageStore {
        let image_store_root =
            std::fs::canonicalize(Path::new(DEFAULT_STORE_ROOT)).map_err(|io_source| {
                JailerError::Io {
                    path: PathBuf::from(DEFAULT_STORE_ROOT),
                    source: io_source,
                }
            })?;
        if !mount_source.starts_with(&image_store_root) {
            return Err(JailerError::BindSourceRejected {
                src: source.to_path_buf(),
                expected_root: PathBuf::from(DEFAULT_STORE_ROOT),
            });
        }
    }
    Ok(mount_source)
}

fn is_proc_fd_path(source: &Path) -> bool {
    let mut components = source.components();
    matches!(components.next(), Some(Component::RootDir))
        && matches!(components.next(), Some(Component::Normal(proc)) if proc == "proc")
        && matches!(components.next(), Some(Component::Normal(_pid)))
        && matches!(components.next(), Some(Component::Normal(fd)) if fd == "fd")
        && components.next().is_some()
        && components.next().is_none()
}

fn bind_mount_flags(source_is_dir: bool, dest_is_dir: bool) -> nix::mount::MsFlags {
    let mut flags = nix::mount::MsFlags::MS_BIND;
    if source_is_dir || dest_is_dir {
        flags |= nix::mount::MsFlags::MS_REC;
    }
    flags
}

fn private_mount_flags() -> nix::mount::MsFlags {
    nix::mount::MsFlags::MS_PRIVATE | nix::mount::MsFlags::MS_REC
}

fn make_mounts_private() -> Result<(), JailerError> {
    nix::mount::mount(
        None::<&str>,
        "/",
        None::<&str>,
        private_mount_flags(),
        None::<&str>,
    )
    .map_err(JailerError::MountPropagationFailed)
}

struct UmaskGuard(nix::sys::stat::Mode);

impl UmaskGuard {
    fn zero() -> Self {
        Self(umask(nix::sys::stat::Mode::empty()))
    }
}

impl Drop for UmaskGuard {
    fn drop(&mut self) {
        umask(self.0);
    }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::types::BindMode;
    use nix::mount::MsFlags;

    #[test]
    fn bind_remount_flags_is_exactly_expected_bitset() {
        let expected = MsFlags::MS_BIND
            | MsFlags::MS_REMOUNT
            | MsFlags::MS_NODEV
            | MsFlags::MS_NOEXEC
            | MsFlags::MS_NOSUID;

        assert_eq!(
            super::bind_remount_flags(),
            expected,
            "bind_remount_flags() bitset changed; see crates/m80-jailer/src/plan.rs:282. \
             MS_BIND is required for remounting a bind mount; the earlier audit \
             regression m80-l020n.9 was wrong to remove it."
        );
    }

    #[test]
    fn shared_image_store_bind_remount_flags_include_readonly() {
        let expected = MsFlags::MS_BIND
            | MsFlags::MS_REMOUNT
            | MsFlags::MS_NODEV
            | MsFlags::MS_NOEXEC
            | MsFlags::MS_NOSUID
            | MsFlags::MS_RDONLY;

        assert_eq!(
            super::bind_remount_flags_for_mode(BindMode::RoImageStore),
            expected,
            "shared image-store binds must remount read-only"
        );
    }

    #[test]
    fn file_bind_mount_flags_omit_recursive_bind() {
        assert_eq!(
            super::bind_mount_flags(false, false),
            MsFlags::MS_BIND,
            "file-to-file bind mounts must not carry MS_REC"
        );
    }

    #[test]
    fn directory_bind_mount_flags_keep_recursive_bind() {
        let expected = MsFlags::MS_BIND | MsFlags::MS_REC;
        assert_eq!(super::bind_mount_flags(true, false), expected);
        assert_eq!(super::bind_mount_flags(false, true), expected);
        assert_eq!(super::bind_mount_flags(true, true), expected);
    }

    #[test]
    fn private_mount_flags_make_recursive_private() {
        let expected = MsFlags::MS_PRIVATE | MsFlags::MS_REC;
        assert_eq!(super::private_mount_flags(), expected);
    }

    #[test]
    fn proc_fd_bind_sources_are_not_canonicalized() {
        let source = Path::new("/proc/123/fd/9");

        assert_eq!(super::bind_mount_source(source).unwrap(), source);
    }
}
