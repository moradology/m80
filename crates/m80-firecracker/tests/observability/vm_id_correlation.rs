const EXEC: &str = include_str!("../../src/lifecycle/exec.rs");
const HOTPLUG: &str = include_str!("../../src/lifecycle/hotplug.rs");
const LAUNCH: &str = include_str!("../../src/launch.rs");
const LIFECYCLE: &str = include_str!("../../src/lifecycle.rs");
const WARM_POOL: &str = include_str!("../../src/warm_pool.rs");
const WARM_POOL_FILL_WORKER: &str = include_str!("../../src/warm_pool/fill_worker.rs");
const WARM_POOL_INNER: &str = include_str!("../../src/warm_pool/inner.rs");
const WARM_LEASE: &str = include_str!("../../src/warm_pool/lease.rs");
const WARM_TEMPLATE_BUILD: &str = include_str!("../../src/warm_pool/template_build.rs");

#[test]
fn public_lifecycle_entrypoints_open_vm_id_spans() {
    assert!(LAUNCH.contains("pub fn launch(self)"));
    assert!(LAUNCH.contains("pub fn launch_from_snapshot("));
    assert!(LIFECYCLE.contains("pub fn capture(&mut self"));
    assert!(LIFECYCLE.contains("pub fn stop(mut self)"));
    assert!(EXEC.contains("pub fn exec(&mut self"));
    assert!(WARM_LEASE.contains("pub fn exec(&mut self"));

    assert!(
        LAUNCH
            .matches("fields(vm_id = %self.config.vm_id.as_deref()")
            .count()
            >= 4,
        "launch and restore paths must carry vm_id spans"
    );
    assert!(
        EXEC.matches("fields(vm_id = %self.vm_id)").count() >= 7,
        "exec entrypoints must carry vm_id spans"
    );
    assert!(
        WARM_LEASE
            .matches("fields(vm_id = %self.current_vm_id())")
            .count()
            >= 4,
        "warm lease exec entrypoints must carry vm_id spans"
    );
}

#[test]
fn teardown_error_events_keep_vm_id_fields() {
    assert_source_contains(
        LIFECYCLE,
        &[
            "unmount_snapshot_bind(&vm_id, snapshot_mount.as_deref())",
            "tracing::warn!(vm_id = %vm_id",
            "vsock graceful-stop failed; SIGKILLing without ack",
            "force_kill failed; leaking sandbox state",
        ],
    );
    assert_source_contains(
        EXEC,
        &[
            "struct CancelForwarder",
            "struct PtyEventForwarder",
            "vm_id: String",
            "tracing::error!(vm_id = %vm_id, ?panic",
            "tracing::error!(vm_id = %vm_id, \"{name} thread did not stop",
        ],
    );
    assert_source_contains(
        HOTPLUG,
        &[
            "let vm_id = sandbox.vm_id.clone();",
            "vm_id = %vm_id",
            "error = %cleanup",
        ],
    );
    assert_source_contains(
        WARM_LEASE,
        &[
            "let vm_id = sandbox.vm_id().to_owned();",
            "failed to discard one-shot warm lease after exec success",
            "failed to discard one-shot warm lease after exec error",
            "failed to discard warm lease during drop",
        ],
    );
    assert_source_contains(
        WARM_POOL,
        &[
            "failed to discard surplus warm-pool slot after resize",
            "failed to discard warm-pool slot during drop",
            "vm_id = %vm_id",
        ],
    );
    assert_source_contains(
        WARM_POOL_INNER,
        &[
            "failed to discard unleased warm-pool slot",
            "vm_id = %vm_id",
        ],
    );
    assert_source_contains(
        WARM_POOL_FILL_WORKER,
        &[
            "failed to discard warm-pool slot after shutdown",
            "vm_id = %vm_id",
        ],
    );
    assert_source_contains(
        WARM_TEMPLATE_BUILD,
        &[
            "failed to discard sandbox after template build failure",
            "vm_id = %vm_id",
        ],
    );
}

fn assert_source_contains(source: &str, needles: &[&str]) {
    for needle in needles {
        assert!(source.contains(needle), "missing source needle: {needle}");
    }
}
