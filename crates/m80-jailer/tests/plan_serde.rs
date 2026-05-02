//! Plan serde round-trip tests.

use m80_jailer::{BindMode, Binding, JailerConfig, Plan, SocketSpec};
use std::path::PathBuf;

fn sample_config() -> JailerConfig {
    JailerConfig {
        jailer_bin: PathBuf::from("/usr/bin/jailer"),
        firecracker_bin: PathBuf::from("/usr/bin/firecracker"),
        run_dir: PathBuf::from("/tmp/run/vm-42"),
        uid: 3000,
        gid: 3000,
        bindings: vec![
            Binding {
                source: PathBuf::from("/images/kernel"),
                dest: PathBuf::from("kernel/vmlinux"),
                mode: BindMode::Ro,
            },
            Binding {
                source: PathBuf::from("/vms/rootfs.ext4"),
                dest: PathBuf::from("drives/rootfs.ext4"),
                mode: BindMode::Rw,
            },
            Binding {
                source: PathBuf::from(""),
                dest: PathBuf::from("run"),
                mode: BindMode::CreateInsideJail,
            },
        ],
        sockets: vec![SocketSpec {
            path: PathBuf::from("run/firecracker.sock"),
        }],
    }
}

#[test]
fn plan_round_trips_via_serde() {
    let cfg = sample_config();
    let plan = Plan::compute(&cfg).unwrap();

    let json = serde_json::to_string(&plan).unwrap();
    let plan2: Plan = serde_json::from_str(&json).unwrap();

    // Verify that step counts match.
    assert_eq!(
        plan.steps.len(),
        plan2.steps.len(),
        "step count must match after round-trip"
    );
    // Verify that uid/gid survive.
    assert_eq!(plan2.config.uid, 3000);
    assert_eq!(plan2.config.gid, 3000);
    assert_eq!(plan2.config.run_dir, PathBuf::from("/tmp/run/vm-42"));
}

#[test]
fn plan_round_trips_byte_equal_via_pretty_json() {
    let cfg = sample_config();
    let plan = Plan::compute(&cfg).unwrap();

    let json1 = serde_json::to_string_pretty(&plan).unwrap();
    let plan2: Plan = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string_pretty(&plan2).unwrap();

    assert_eq!(json1, json2, "pretty-print round-trip must be byte-equal");
}

#[test]
fn plan_step_kinds_tagged_correctly_in_json() {
    let cfg = sample_config();
    let plan = Plan::compute(&cfg).unwrap();

    let json = serde_json::to_string(&plan).unwrap();
    // Each step variant should be tagged with "kind".
    assert!(
        json.contains("\"kind\":\"create_dir\"") || json.contains("\"kind\": \"create_dir\""),
        "CreateDir step must serialize with kind=create_dir; json={json}"
    );
    assert!(
        json.contains("\"kind\":\"bind\"") || json.contains("\"kind\": \"bind\""),
        "Bind step must serialize with kind=bind; json={json}"
    );
    assert!(
        json.contains("\"kind\":\"socket\"") || json.contains("\"kind\": \"socket\""),
        "Socket step must serialize with kind=socket; json={json}"
    );
}

#[test]
fn bind_mode_serializes_as_snake_case() {
    let ro = serde_json::to_string(&BindMode::Ro).unwrap();
    let rw = serde_json::to_string(&BindMode::Rw).unwrap();
    let cij = serde_json::to_string(&BindMode::CreateInsideJail).unwrap();

    assert_eq!(ro, "\"ro\"");
    assert_eq!(rw, "\"rw\"");
    assert_eq!(cij, "\"create_inside_jail\"");
}
