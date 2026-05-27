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
#[cfg(feature = "_test_internal")]
#[doc(hidden)]
pub use materialized::materialized_jail_for_test;
pub use materialized::{JailedFirecracker, MaterializedJail};
pub use recover::{inspect_run_dir, InspectionDecision, ReapPlan};
pub use types::{
    jail_root_path, BindMode, Binding, CgroupVersion, JailerConfig, JailerSocket, Plan,
    ResourceLimits, JAILER_PLAN_FILE, JAILER_STATE_FILE, JAILER_SYSTEMD_UNIT_FILE,
};
