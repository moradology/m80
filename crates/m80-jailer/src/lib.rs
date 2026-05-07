//! Materialize the Firecracker jailer chroot per VM with a replayable plan.
//!
//! See `README.md` for the black-box contract.
//! Behavior captures: bead epic `m80-zmg`.

#![deny(missing_docs)]

mod error;
mod materialized;
mod plan;
mod recover;
mod types;

pub use error::JailerError;
pub use materialized::{JailedFirecracker, MaterializedJail};
pub use recover::{inspect_run_dir, InspectionDecision};
pub use types::{
    jail_root_path, BindMode, Binding, JailerConfig, Plan, PlanStep, SocketSpec, JAILER_PLAN_FILE,
    JAILER_STATE_FILE,
};
