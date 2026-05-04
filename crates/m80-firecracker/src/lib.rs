//! VM lifecycle state machine: composes the m80 foundation crates into a
//! launchable sandbox. See `README.md` for the black-box contract.

#![deny(missing_docs)]

mod backend;
mod config;
mod error;
mod launch;
mod lifecycle;
mod runroot;
mod types;

pub use error::FcError;
pub use types::{
    Backend, BackendConfig, CgroupMode, ConfigSource, EffectiveConfig, EffectiveField,
    RunningSandbox, Sandbox, SandboxConfig, StoppedSandbox,
};
pub use m80_net_mode::NetworkPolicy;
pub use m80_proto::{ExecRequest, ExecResponse, ExecStatus, ExecTiming};
pub use config::{backend_config_from_effective, load as load_config};
pub use m80_storage::ChangeSet;
pub use runroot::OWNERSHIP_LOCK;
