//! VM lifecycle state machine: composes the m80 foundation crates into a
//! launchable sandbox. See `README.md` for the black-box contract.

#![deny(missing_docs)]

mod backend;
mod config;
mod diagnostics;
mod error;
mod launch;
mod lifecycle;
mod runroot;
mod timing;
mod types;
mod warm_pool;

pub use config::{
    backend_config_from_effective, load as load_config, load_from_paths as load_config_from_paths,
    ConfigFilePaths,
};
pub use error::{FcError, LifecycleFailureKind};
pub use m80_net_mode::NetworkPolicy;
pub use m80_proto::{
    ExecExit, ExecRequest, ExecResponse, ExecStatus, ExecTiming, PtyControlEvent, PtyExit,
    PtyRequest, PtySize,
};
pub use m80_snapshot::SnapshotPaths;
pub use m80_storage::ChangeSet;
pub use runroot::OWNERSHIP_LOCK;
pub use types::{
    Backend, BackendConfig, CgroupMode, ConfigSource, EffectiveConfig, EffectiveField, ExecChunk,
    PtyHostEvent, PtyOutputChunk, RunningSandbox, Sandbox, SandboxConfig, StoppedSandbox,
};
pub use warm_pool::{
    BlankVmResetDecision, BlankVmResetDiscardReason, BlankVmResetEvidence, WarmLease, WarmPool,
    WarmPoolConfig, WarmPoolSnapshot,
};
