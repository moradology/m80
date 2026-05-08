//! Attack implementations grouped by defense-in-depth category.

pub(crate) mod cross_tenant;
pub(crate) mod filesystem;
pub(crate) mod network;
pub(crate) mod privilege;
pub(crate) mod process;
pub(crate) mod resource;
