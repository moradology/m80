//! Bounded resource-exhaustion attack attempts.

use std::fs::File;
use std::thread;

use crate::{blocked, AttackBlocked, AttackResult};

pub(crate) fn open_many_file_descriptors() -> AttackResult {
    let mut files = Vec::new();
    for _ in 0..4096 {
        match File::open("/dev/null") {
            Ok(file) => files.push(file),
            Err(err) => return Err(blocked("open /dev/null repeatedly", err)),
        }
    }
    Ok(())
}

pub(crate) fn spawn_many_threads() -> AttackResult {
    let mut handles = Vec::new();
    for _ in 0..256 {
        match thread::Builder::new().spawn(thread::park) {
            Ok(handle) => handles.push(handle),
            Err(err) => return Err(blocked("spawn thread", err)),
        }
    }
    Ok(())
}

pub(crate) fn allocate_large_memory() -> AttackResult {
    let allocation = Vec::<u8>::with_capacity(1024 * 1024 * 1024);
    if allocation.capacity() >= 1024 * 1024 * 1024 {
        Ok(())
    } else {
        Err(AttackBlocked::new("allocator returned smaller capacity"))
    }
}

pub(crate) fn create_large_tmp_file() -> AttackResult {
    let path = "/tmp/m80-attack-runner-large-file";
    let file = File::create(path).map_err(|err| blocked(format!("create {path}"), err))?;
    file.set_len(1024 * 1024 * 1024)
        .map_err(|err| blocked(format!("set_len {path}"), err))
}
