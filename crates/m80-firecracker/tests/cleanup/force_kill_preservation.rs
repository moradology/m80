use std::path::PathBuf;

use m80_firecracker::{FcError, RunningSandbox, StoppedSandbox};

#[test]
fn force_kill_is_separate_last_resort_surface() {
    let _: fn(RunningSandbox) -> Result<StoppedSandbox, FcError> = RunningSandbox::stop;
    let _: fn(RunningSandbox) -> Result<StoppedSandbox, FcError> = RunningSandbox::force_kill;
}

#[test]
fn run_dir_preservation_is_explicit_after_stop() {
    let _: fn(StoppedSandbox) -> Result<(), FcError> = StoppedSandbox::delete;
    let _: fn(StoppedSandbox) -> Result<PathBuf, FcError> = StoppedSandbox::preserve_for_triage;
}
