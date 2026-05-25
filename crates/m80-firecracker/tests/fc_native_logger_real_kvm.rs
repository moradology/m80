//! Real-KVM check that Firecracker's native logger is configured.

mod common;

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn fc_native_logger_file_exists_after_launch() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let firecracker_bin = discovery.firecracker_bin.clone();
    let backend = common::make_backend(discovery);
    let vm_id = common::unique_vm_id("fc-log");
    let sandbox = backend
        .admit(common::sandbox_config_with_id(vm_id))
        .expect("admit");
    let running = sandbox.launch().expect("launch");
    let run_dir = running.run_dir().to_path_buf();
    let _dump = common::RunDirDumpGuard::new(run_dir.clone());

    let log_path = m80_firecracker::fc_log_path(&run_dir, &firecracker_bin);
    assert!(
        log_path.is_file(),
        "Firecracker native log file missing at {}",
        log_path.display()
    );

    let stopped = running.stop().expect("stop");
    stopped.delete().expect("delete");
}
