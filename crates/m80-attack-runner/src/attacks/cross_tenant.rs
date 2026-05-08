//! Cross-tenant isolation attack attempts.

use std::fs;

use crate::{blocked, peer_sentinel, AttackBlocked, AttackResult};

pub(crate) fn read_peer_sentinel() -> AttackResult {
    read_forbidden(&peer_sentinel(), "peer sentinel readable")
}

pub(crate) fn write_peer_sentinel() -> AttackResult {
    let path = peer_sentinel();
    fs::write(&path, b"m80 attack runner touched peer sentinel\n")
        .map_err(|err| blocked(format!("write {path}"), err))
}

pub(crate) fn list_peer_run_dir() -> AttackResult {
    let path =
        std::env::var("M80_ATTACK_PEER_RUN_DIR").unwrap_or_else(|_| "/run/m80/peer".to_owned());
    let count = fs::read_dir(&path)
        .map_err(|err| blocked(format!("read_dir {path}"), err))?
        .filter_map(Result::ok)
        .take(2)
        .count();
    if count > 0 {
        Ok(())
    } else {
        Err(AttackBlocked::new(format!("{path} listed but was empty")))
    }
}

pub(crate) fn read_peer_network_state() -> AttackResult {
    let path = std::env::var("M80_ATTACK_PEER_NETWORK_STATE")
        .unwrap_or_else(|_| "/run/m80/peer/network-state.json".to_owned());
    read_forbidden(&path, "peer network state readable")
}

fn read_forbidden(path: &str, context: &'static str) -> AttackResult {
    match fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => Ok(()),
        Ok(_) => Err(AttackBlocked::new(format!("{context}: empty file"))),
        Err(err) => Err(blocked(format!("read {path}"), err)),
    }
}
