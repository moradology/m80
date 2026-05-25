//! Real-KVM smoke for rootfs overlay-template clone divergence.
//!
//! Run through `scripts/smoke.sh` with:
//!
//! ```text
//! M80_VERIFY_REFLINK_DIVERGENCE=1 ./scripts/smoke.sh launch-only
//! ```

use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use m80_proto::{ExecRequest, ExecStatus};

mod common;

const WRITE_MIB: u64 = 16;
const MIN_DIVERGENCE_BLOCKS: u64 = 30_000;

#[test]
#[ignore = "requires-kvm requires-artifacts"]
fn reflink_rootfs_real_kvm_boot_write_diverges_overlay_from_template() {
    let discovery =
        m80_preflight::run().expect("preflight must pass on a KVM-capable host with m80 artifacts");
    let backend = common::make_backend(discovery);
    let vm_id = common::unique_vm_id("reflink-div");
    let overlay_size = common::sandbox_config().overlay_size_bytes;
    let run_dir = backend.config().run_root().join(&vm_id);
    let overlay = run_dir.join("rootfs.overlay.ext4");
    let template = backend
        .config()
        .run_root()
        .join(format!(".rootfs-overlay-template-v1-{overlay_size}.ext4"));

    let sandbox = backend
        .admit(m80_firecracker::SandboxConfig {
            vm_id: Some(vm_id.clone()),
            ..common::sandbox_config()
        })
        .expect("admit");
    let mut running = sandbox.launch().expect("launch");
    let _dump_guard = common::RunDirDumpGuard::new(run_dir.clone());
    assert!(
        running
            .create_dir("/tmp/m80-divergence", Some(0o777), false)
            .expect("create exec-writable divergence directory"),
        "divergence directory should be newly created"
    );

    let overlay_before = allocated_blocks(&overlay);
    let template_before = allocated_blocks(&template);
    let overlay_frag_before = filefrag_head(&overlay);
    let template_frag_before = filefrag_head(&template);

    let response = running
        .exec(ExecRequest {
            program: "/bin/sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                format!(
                    "dd if=/dev/urandom of=/tmp/m80-divergence/payload.bin bs=1M count={WRITE_MIB} && sync"
                ),
            ],
            cwd: None,
            env: None,
            stdin: None,
            timeout_ms: Some(20_000),
            streaming: false,
        })
        .expect("write divergence marker inside guest");
    assert_eq!(response.status, ExecStatus::Completed);
    assert_eq!(
        response.exit_code,
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&response.stdout),
        String::from_utf8_lossy(&response.stderr)
    );

    let stopped = running.stop().expect("stop");

    let overlay_after = wait_for_allocated_blocks_at_least(
        &overlay,
        overlay_before + MIN_DIVERGENCE_BLOCKS,
        Duration::from_secs(10),
    );
    let template_after = allocated_blocks(&template);
    let overlay_frag_after = filefrag_head(&overlay);
    let template_frag_after = filefrag_head(&template);
    let overlay_delta = overlay_after.saturating_sub(overlay_before);

    println!("overlay_blocks_before={overlay_before}");
    println!("overlay_blocks_after={overlay_after}");
    println!("template_blocks_before={template_before}");
    println!("template_blocks_after={template_after}");
    println!("overlay_filefrag_before={overlay_frag_before}");
    println!("overlay_filefrag_after={overlay_frag_after}");
    println!("template_filefrag_before={template_frag_before}");
    println!("template_filefrag_after={template_frag_after}");

    assert_eq!(
        template_after, template_before,
        "template allocation must not change when guest writes to overlay"
    );
    assert!(
        overlay_delta >= MIN_DIVERGENCE_BLOCKS,
        "overlay allocation must grow by at least {MIN_DIVERGENCE_BLOCKS} blocks, got {overlay_delta}"
    );

    stopped.delete().expect("delete");
    println!(
        "REFLINK_DIVERGENCE_OK vm_id={vm_id} run_dir={} overlay_delta_blocks={overlay_delta}",
        run_dir.display()
    );
}

fn allocated_blocks(path: &Path) -> u64 {
    std::fs::metadata(path)
        .unwrap_or_else(|err| panic!("metadata {}: {err}", path.display()))
        .blocks()
}

fn wait_for_allocated_blocks_at_least(path: &Path, min_blocks: u64, timeout: Duration) -> u64 {
    let start = Instant::now();
    loop {
        let blocks = allocated_blocks(path);
        if blocks >= min_blocks || start.elapsed() >= timeout {
            return blocks;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn filefrag_head(path: &Path) -> String {
    let output = Command::new("filefrag").arg("-v").arg(path).output();
    let output = match output {
        Ok(output) => output,
        Err(err) => return format!("filefrag unavailable for {}: {err}", path.display()),
    };
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    text.lines().take(5).collect::<Vec<_>>().join(" | ")
}
