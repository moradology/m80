//! Smoke tests for the `m80 template` operator command group.

mod common;

use std::path::Path;

use common::m80;
use m80_snapshot_template::{
    GuestMountPath, HookSpecSet, ImageDigest, JailBackingPath, PmemTemplateEntry,
    PmemTemplateSharing, TemplateDigest, TemplateInputs, TemplateRestoreLayout, TemplateStore,
};
use serde_json::Value;

#[test]
fn template_lifecycle_renders_human_and_json() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store_root = temp.path().join("templates");
    let fingerprint = commit_template(&store_root);

    let list_json = command_json([
        "--json",
        "template",
        "list",
        "--store",
        store_root.to_str().expect("utf8 store"),
    ]);
    assert_eq!(
        list_json["data"]["templates"][0]["fingerprint"],
        fingerprint
    );

    let human_list = m80()
        .args(["template", "list", "--store", store_root.to_str().unwrap()])
        .output()
        .expect("human list");
    assert!(human_list.status.success());
    let stdout = String::from_utf8(human_list.stdout).expect("utf8 stdout");
    assert!(stdout.contains("FINGERPRINT\tSIZE_BYTES\tLAST_USED_UNIX_MS"));
    assert!(stdout.contains(&fingerprint));

    let show_json = command_json([
        "--json",
        "template",
        "show",
        &fingerprint,
        "--store",
        store_root.to_str().unwrap(),
    ]);
    assert_eq!(show_json["data"]["fingerprint"], fingerprint);
    assert_eq!(show_json["data"]["manifest"]["fingerprint"], fingerprint);

    let human_show = m80()
        .args([
            "template",
            "show",
            &fingerprint,
            "--store",
            store_root.to_str().unwrap(),
        ])
        .output()
        .expect("human show");
    assert!(human_show.status.success());
    let stdout = String::from_utf8(human_show.stdout).expect("utf8 stdout");
    assert!(stdout.contains(&format!("fingerprint: {fingerprint}")));
    assert!(stdout.contains("schema_version: 1"));

    let prune_json = command_json([
        "--json",
        "template",
        "prune",
        "--store",
        store_root.to_str().unwrap(),
    ]);
    assert_eq!(prune_json["data"]["removed_count"], 0);

    let rm_json = command_json([
        "--json",
        "template",
        "rm",
        &fingerprint,
        "--store",
        store_root.to_str().unwrap(),
    ]);
    assert_eq!(rm_json["data"]["fingerprint"], fingerprint);
    assert_eq!(rm_json["data"]["removed"], true);

    let list_after = command_json([
        "--json",
        "template",
        "list",
        "--store",
        store_root.to_str().unwrap(),
    ]);
    assert_eq!(list_after["data"]["templates"].as_array().unwrap().len(), 0);
}

#[test]
fn template_build_rejects_boot_spec_parse_error_before_host_action() {
    let temp = tempfile::tempdir().expect("tempdir");
    let boot_spec = temp.path().join("bad.yaml");
    std::fs::write(&boot_spec, "schema_version: [").expect("bad boot spec");

    let output = m80()
        .args([
            "--json",
            "template",
            "build",
            "fixture",
            "--boot-spec",
            boot_spec.to_str().unwrap(),
        ])
        .output()
        .expect("template build");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let error: Value = serde_json::from_str(&stderr).expect("json error");
    assert_eq!(error["data"]["variant"], "Config");
    assert!(error["data"]["detail"]
        .as_str()
        .unwrap()
        .contains("boot_spec.yaml"));
}

#[test]
fn template_prune_rejects_boot_fill_boot_spec_before_host_action() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store_root = temp.path().join("templates");
    commit_template(&store_root);
    let boot_spec = temp.path().join("boot-spec.yaml");
    std::fs::write(&boot_spec, valid_boot_fill_boot_spec()).expect("boot spec");

    let output = m80()
        .args([
            "--json",
            "template",
            "prune",
            "--store",
            store_root.to_str().unwrap(),
            "--boot-spec",
            boot_spec.to_str().unwrap(),
        ])
        .output()
        .expect("template prune");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let error: Value = serde_json::from_str(&stderr).expect("json error");
    assert_eq!(error["data"]["variant"], "Config");
    assert!(error["data"]["detail"]
        .as_str()
        .unwrap()
        .contains("template prune requires snapshot_restore"));
}

fn command_json<const N: usize>(args: [&str; N]) -> Value {
    let output = m80().args(args).output().expect("m80 command");
    assert!(
        output.status.success(),
        "command failed: status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("json stdout")
}

fn commit_template(root: &Path) -> String {
    let store = TemplateStore::create(root, 4).expect("template store");
    let inputs = TemplateInputs::new(
        "6.17.0",
        "v1.15.1",
        digest("a"),
        vec![PmemTemplateEntry::new(
            GuestMountPath::parse("/opt/m80-layers/toolchain").expect("mount path"),
            ImageDigest::parse(&"c".repeat(64)).expect("image digest"),
            PmemTemplateSharing::Shared,
            JailBackingPath::parse("/pmem/toolchain.erofs").expect("jail path"),
        )],
        digest("b"),
        HookSpecSet::empty(),
    )
    .expect("template inputs");
    let layout = TemplateRestoreLayout::new(
        JailBackingPath::parse("/snapshot/vm.snap").expect("vm path"),
        JailBackingPath::parse("/snapshot/mem.snap").expect("mem path"),
        inputs.pmem_image_digest_set().to_vec(),
    );
    let plan = store.reserve(inputs, layout).expect("reserve template");
    std::fs::write(&plan.body_paths().vm_state, b"vm").expect("vm body");
    std::fs::write(&plan.body_paths().mem, b"mem").expect("mem body");
    let pinned = store.commit(plan).expect("commit template");
    let fingerprint = pinned.reference().fingerprint().to_hex();
    drop(pinned);
    fingerprint
}

fn digest(byte: &str) -> TemplateDigest {
    TemplateDigest::parse(&byte.repeat(64)).expect("digest")
}

fn valid_boot_fill_boot_spec() -> &'static str {
    r#"schema_version: 1
name: test-template
sandbox:
  vm_id_prefix: test-template
  vcpu_count: 1
  mem_size_mib: 512
  network: none
  overlay_size_bytes: 536870912
pmem_layers: []
warm_strategy:
  mode: boot_fill
"#
}
