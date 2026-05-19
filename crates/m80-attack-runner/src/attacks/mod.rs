//! Attack implementations grouped by defense-in-depth category.

use std::fs;

use crate::{blocked, AttackBlocked, AttackResult};

pub(crate) mod cross_tenant;
pub(crate) mod filesystem;
pub(crate) mod network;
pub(crate) mod privilege;
pub(crate) mod process;
pub(crate) mod resource;

fn read_forbidden(path: &str, context: &'static str) -> AttackResult {
    match fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => Ok(()),
        Ok(_) => Err(AttackBlocked::new(format!("{context}: empty file"))),
        Err(err) => Err(blocked(format!("read {path}"), err)),
    }
}
