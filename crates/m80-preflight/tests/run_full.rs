//! Full smoke test: actually calls `run()` on the host.
//!
//! This test is `#[ignore]` by default because it requires:
//! - A Linux host with `/dev/kvm` available and writable.
//! - `vmx` or `svm` advertised in `/proc/cpuinfo`.
//! - `bridge` and `tap` kernel modules loaded.
//! - Root or the required Linux capabilities in the effective set.
//! - Firecracker and jailer binaries installed (or env overrides set).
//! - A kernel image and rootfs available (env overrides or default paths).
//! - `/var/run/m80` (or M80_RUN_ROOT) to exist with >= 100 MiB free.
//! - `mkfs.ext4`, `cp`, `fallocate`, `e2fsck`, `debugfs` on PATH.
//!
//! Run explicitly with:
//! ```text
//! cargo test -p m80-preflight -- --ignored run_on_kvm_host
//! ```

#[test]
#[ignore = "requires KVM-capable Linux host with all m80 artifacts installed"]
fn run_on_kvm_host() {
    let result = m80_preflight::run();
    match &result {
        Ok(d) => {
            eprintln!("Preflight passed. Table:\n{}", d.render_table());
            let labels = d
                .report
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                labels,
                vec![
                    "OS gate",
                    "KVM",
                    "KVM CPU extensions",
                    "Kernel modules",
                    "Transparent hugepages",
                    "KVM halt polling",
                    "CPU governor",
                    "Cgroup mode",
                    "Privilege",
                    "Firecracker binary",
                    "Jailer binary",
                    "Jailer hardening wrapper",
                    "Kernel image",
                    "Rootfs + manifest",
                    "Run-root",
                    "Run-root filesystem",
                    "Storage helpers",
                ]
            );
            for row in &d.report {
                assert!(
                    row.passed,
                    "check '{}' must have passed; detail: {}",
                    row.label, row.detail
                );
            }
        }
        Err(e) => {
            panic!("preflight failed: {e}");
        }
    }
}
