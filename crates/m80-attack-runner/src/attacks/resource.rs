//! Bounded resource-exhaustion attack attempts.

use std::fs::File;
use std::io::Write;
use std::thread;

use crate::{blocked, AttackResult};

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
    let mut chunks = Vec::new();
    for index in 0..512 {
        let mut chunk = vec![0_u8; 1024 * 1024];
        let last = chunk.len() - 1;
        chunk[0] = index as u8;
        chunk[last] = index as u8;
        chunks.push(chunk);
    }
    Ok(())
}

pub(crate) fn create_large_tmp_file() -> AttackResult {
    // Stay cwd-relative so the jailer harness controls which mounted layer and
    // quota boundary receive the write.
    let path = "m80-attack-runner-large-file";
    let mut file = File::create(path).map_err(|err| blocked(format!("create {path}"), err))?;
    let chunk = [0xA5_u8; 64 * 1024];
    for _ in 0..4096 {
        file.write_all(&chunk)
            .map_err(|err| blocked(format!("write {path}"), err))?;
    }
    Ok(())
}
