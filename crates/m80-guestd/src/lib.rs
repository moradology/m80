//! Library target for `m80-guestd` — exposes the connection handler for
//! integration tests. No public API beyond test support.

pub mod connection;
#[allow(dead_code)]
mod exec_sandbox;
pub mod guest_log;
#[allow(dead_code)]
mod guest_seccomp;
pub(crate) mod uevent;
mod workload_broker;
