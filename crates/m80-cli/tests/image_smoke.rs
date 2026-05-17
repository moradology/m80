//! Smoke tests for the `m80 image` operator command group.

mod common;

use std::path::Path;

use common::m80;
use m80_image_store::{ImageDigest as StoreImageDigest, ImageStore};
use m80_snapshot_template::{
    GuestMountPath, HookSpecSet, ImageDigest as TemplateImageDigest, JailBackingPath,
    PmemTemplateEntry, PmemTemplateSharing, TemplateDigest, TemplateInputs, TemplateRestoreLayout,
    TemplateStore,
};
use serde_json::Value;

#[test]
fn image_lifecycle_renders_human_and_json() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = temp.path().join("images");
    let source = temp.path().join("source.ext4");
    std::fs::write(&source, b"image bytes").expect("source image");

    let digest = image_build_json(&source, &store);

    let list_json = command_json([
        "--json",
        "image",
        "list",
        "--store",
        store.to_str().expect("utf8 store"),
    ]);
    assert_eq!(list_json["data"]["images"][0]["digest"], digest);
    assert_eq!(list_json["data"]["images"][0]["kind"], "ext4");

    let human_list = m80()
        .args(["image", "list", "--store", store.to_str().unwrap()])
        .output()
        .expect("human list");
    assert!(human_list.status.success());
    let stdout = String::from_utf8(human_list.stdout).expect("utf8 stdout");
    assert!(stdout.contains("KIND\tDIGEST\tSIZE_BYTES\tPATH"));
    assert!(stdout.contains(&digest));

    let show_json = command_json([
        "--json",
        "image",
        "show",
        &digest,
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(show_json["data"]["digest"], digest);
    assert_eq!(show_json["data"]["images"][0]["size_bytes"], 11);

    let verify_json = command_json([
        "--json",
        "image",
        "verify",
        &digest,
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(verify_json["data"]["verified"], true);

    let missing_templates = temp.path().join("missing-templates");
    let rm_json = command_json([
        "--json",
        "image",
        "rm",
        &digest,
        "--store",
        store.to_str().unwrap(),
        "--template-store",
        missing_templates.to_str().expect("utf8 template store"),
    ]);
    assert_eq!(rm_json["data"]["removed"][0]["digest"], digest);

    let list_after = command_json([
        "--json",
        "image",
        "list",
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(list_after["data"]["images"].as_array().unwrap().len(), 0);
}

#[test]
fn image_rm_refuses_template_references() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = temp.path().join("images");
    let source = temp.path().join("source.ext4");
    std::fs::write(&source, b"referenced image bytes").expect("source image");
    let digest = image_build_json(&source, &store);

    let template_store = temp.path().join("templates");
    commit_template_reference(&template_store, &digest);

    let output = m80()
        .args([
            "--json",
            "image",
            "rm",
            &digest,
            "--store",
            store.to_str().unwrap(),
            "--template-store",
            template_store.to_str().unwrap(),
        ])
        .output()
        .expect("image rm");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let error: Value = serde_json::from_str(&stderr).expect("json error");
    assert_eq!(error["data"]["variant"], "ImageStore");
    assert!(error["data"]["detail"]
        .as_str()
        .unwrap()
        .contains("referenced by snapshot templates"));

    command_json([
        "--json",
        "image",
        "show",
        &digest,
        "--store",
        store.to_str().unwrap(),
    ]);
}

#[test]
fn image_gc_dry_run_reports_candidates_and_protected_images() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store_root = temp.path().join("images");
    let template_store = temp.path().join("templates");
    let candidate = image_build_bytes(temp.path(), &store_root, "candidate", b"candidate");
    let keep = image_build_bytes(temp.path(), &store_root, "keep", b"keep");
    let pinned = image_build_bytes(temp.path(), &store_root, "pinned", b"pinned");
    let shared = image_build_bytes(temp.path(), &store_root, "shared", b"shared");
    let templated = image_build_bytes(temp.path(), &store_root, "templated", b"templated");
    let pin_file = temp.path().join("pins.txt");
    std::fs::write(&pin_file, format!("{pinned}\n")).expect("pin file");

    let store = ImageStore::open(&store_root).expect("image store");
    let shared_digest = StoreImageDigest::parse(&shared).expect("shared digest");
    let _shared_ref = store
        .acquire_shared_ref(&shared_digest, "vm-shared")
        .expect("shared ref");
    commit_template_reference(&template_store, &templated);

    let gc = command_json([
        "--json",
        "image",
        "gc",
        "--store",
        store_root.to_str().unwrap(),
        "--template-store",
        template_store.to_str().unwrap(),
        "--keep",
        &keep,
        "--pin-file",
        pin_file.to_str().unwrap(),
    ]);

    assert_eq!(gc["data"]["dry_run"], true);
    assert_eq!(gc["data"]["execute"], false);
    assert_eq!(gc["data"]["total_reclaimable_bytes"], 9);
    assert_eq!(gc_entry(&gc, &candidate)["status"], "candidate");
    assert_reasons(gc_entry(&gc, &candidate), []);
    assert_eq!(gc_entry(&gc, &keep)["status"], "protected");
    assert_reasons(gc_entry(&gc, &keep), ["keep"]);
    assert_eq!(gc_entry(&gc, &pinned)["status"], "protected");
    assert_reasons(gc_entry(&gc, &pinned), ["pin-file"]);
    assert_eq!(gc_entry(&gc, &shared)["status"], "protected");
    assert_reasons(gc_entry(&gc, &shared), ["shared-ref:1"]);
    assert_eq!(gc_entry(&gc, &templated)["status"], "protected");
    assert_reason_prefix(gc_entry(&gc, &templated), "template:");

    command_json([
        "--json",
        "image",
        "show",
        &candidate,
        "--store",
        store_root.to_str().unwrap(),
    ]);
}

#[test]
fn image_gc_min_age_retains_new_artifacts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = temp.path().join("images");
    let digest = image_build_bytes(temp.path(), &store, "fresh", b"fresh");

    let gc = command_json([
        "--json",
        "image",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--min-age",
        "1d",
    ]);

    let entry = gc_entry(&gc, &digest);
    assert_eq!(entry["status"], "protected");
    assert_reasons(entry, ["min-age:1d"]);
    assert_eq!(gc["data"]["total_reclaimable_bytes"], 0);
}

#[test]
fn image_gc_execute_removes_only_candidates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = temp.path().join("images");
    let candidate = image_build_bytes(temp.path(), &store, "candidate", b"candidate");
    let keep = image_build_bytes(temp.path(), &store, "keep", b"keep");

    let gc = command_json([
        "--json",
        "image",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--keep",
        &keep,
        "--execute",
    ]);

    assert_eq!(gc["data"]["dry_run"], false);
    assert_eq!(gc["data"]["execute"], true);
    assert_eq!(gc["data"]["total_reclaimable_bytes"], 9);
    assert_eq!(gc["data"]["total_removed_bytes"], 9);
    assert_eq!(gc_entry(&gc, &candidate)["status"], "removed");
    assert_eq!(gc_entry(&gc, &keep)["status"], "protected");

    let missing = m80()
        .args([
            "--json",
            "image",
            "show",
            &candidate,
            "--store",
            store.to_str().unwrap(),
        ])
        .output()
        .expect("show removed image");
    assert!(!missing.status.success());
    command_json([
        "--json",
        "image",
        "show",
        &keep,
        "--store",
        store.to_str().unwrap(),
    ]);
}

fn image_build_json(source: &Path, store: &Path) -> String {
    let value = command_json([
        "--json",
        "image",
        "build",
        "fixture",
        "--source",
        source.to_str().expect("utf8 source"),
        "--out",
        store.to_str().expect("utf8 store"),
        "--kind",
        "ext4",
    ]);
    value["data"]["digest"].as_str().unwrap().to_owned()
}

fn image_build_bytes(parent: &Path, store: &Path, name: &str, bytes: &[u8]) -> String {
    let source = parent.join(format!("{name}.ext4"));
    std::fs::write(&source, bytes).expect("source image");
    image_build_json(&source, store)
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

fn commit_template_reference(root: &Path, image_digest: &str) {
    let store = TemplateStore::create(root, 4).expect("template store");
    let inputs = TemplateInputs::new(
        "6.17.0",
        "v1.15.1",
        digest("a"),
        vec![PmemTemplateEntry::new(
            GuestMountPath::parse("/opt/m80-layers/toolchain").expect("mount path"),
            TemplateImageDigest::parse(image_digest).expect("image digest"),
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
    drop(pinned);
}

fn gc_entry<'a>(value: &'a Value, digest: &str) -> &'a Value {
    value["data"]["images"]
        .as_array()
        .expect("gc image array")
        .iter()
        .find(|entry| entry["digest"] == digest)
        .expect("gc entry")
}

fn assert_reasons<const N: usize>(entry: &Value, expected: [&str; N]) {
    let got: Vec<_> = entry["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|reason| reason.as_str().expect("reason string"))
        .collect();
    assert_eq!(got, expected.as_slice());
}

fn assert_reason_prefix(entry: &Value, prefix: &str) {
    let reasons = entry["reasons"].as_array().expect("reasons");
    assert!(
        reasons.iter().any(|reason| reason
            .as_str()
            .is_some_and(|value| value.starts_with(prefix))),
        "entry {entry:?} did not include reason with prefix {prefix:?}"
    );
}

fn digest(byte: &str) -> TemplateDigest {
    TemplateDigest::parse(&byte.repeat(64)).expect("digest")
}
