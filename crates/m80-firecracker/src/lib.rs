//! VM lifecycle state machine: composes the m80 foundation crates into a
//! launchable sandbox. See `README.md` for the black-box contract.

#![deny(missing_docs)]

mod backend;
mod boot_identity;
mod config;
mod diagnostics;
mod error;
mod hotplug_types;
mod launch;
mod layout;
mod lifecycle;
mod preboot;
mod runroot;
mod storage_prep;
mod types;
mod warm_pool;

pub use config::{
    backend_config_from_effective, load as load_config, load_from_paths as load_config_from_paths,
    ConfigFilePaths,
};
pub use error::{
    CleanupAuthority, CleanupPhase, CleanupReleaseBlocker, ConfigError, FcError,
    LifecycleFailureKind, StopDisposition, WireProtocolError, CLEANUP_AUTHORITY,
    CLEANUP_PHASE_ORDER, CLEANUP_RELEASE_BLOCKERS, STOP_DISPOSITIONS,
};
pub use hotplug_types::{HotplugDriveAttach, HotplugDriveDetach};
pub use layout::{
    boot_identity_path, console_log_path, firecracker_api_socket_path, rootfs_overlay_path,
    run_dir_path, scratch_image_path, vsock_socket_path, BOOT_IDENTITY_FILE, CONSOLE_LOG,
    FIRECRACKER_API_SOCKET, ROOTFS_OVERLAY_IMAGE, SCRATCH_IMAGE, VSOCK_SOCKET,
};
pub use m80_net_mode::NetworkPolicy;
pub use m80_proto::{
    ExecExit, ExecRequest, ExecResponse, ExecStatus, ExecTiming, FileError, FileMkdirRequest,
    FileMkdirResponse, PongResponse, PtyControlEvent, PtyExit, PtyRequest, PtySize,
};
pub use m80_snapshot::SnapshotPaths;
pub use m80_storage::ChangeSet;
pub use runroot::OWNERSHIP_LOCK;
pub use types::{
    Backend, BackendConfig, CgroupMode, ConfigSource, EffectiveConfig, EffectiveField, ExecChunk,
    PtyHostEvent, PtyOutputChunk, RunningSandbox, Sandbox, SandboxConfig, StoppedSandbox,
    DEFAULT_PREALLOCATED_DRIVE_SLOTS, FIRST_LINE_MEM_SIZE_MIB, FIRST_LINE_VCPU_COUNT,
};
pub use warm_pool::{WarmLease, WarmPool, WarmPoolConfig, WarmPoolSnapshot};
