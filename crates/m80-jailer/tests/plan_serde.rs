//! Plan serde round-trip tests.

mod common;

use m80_jailer::{BindMode, Binding, JailerSocket, Plan};
use std::path::{Path, PathBuf};

fn sample_plan() -> Plan {
    let mut cfg = common::minimal_config(Path::new("/tmp/run/vm-42"));
    cfg.bindings = vec![
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
    ];
    cfg.sockets = vec![JailerSocket::Firecracker];
    Plan::compute(&cfg).unwrap()
}

#[test]
fn plan_round_trips_byte_equal_via_pretty_json() {
    let plan = sample_plan();
    let json1 = serde_json::to_string_pretty(&plan).unwrap();
    let plan2: Plan = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string_pretty(&plan2).unwrap();
    assert_eq!(json1, json2, "pretty-print round-trip must be byte-equal");
}

#[test]
fn plan_step_kinds_tagged_correctly_in_json() {
    let plan = sample_plan();
    let v: serde_json::Value = serde_json::to_value(&plan).unwrap();
    let kinds: Vec<&str> = v["steps"]
        .as_array()
        .expect("steps is an array")
        .iter()
        .map(|step| step["kind"].as_str().expect("kind is a string"))
        .collect();
    assert!(
        kinds.contains(&"create_dir"),
        "missing create_dir kind: {kinds:?}"
    );
    assert!(kinds.contains(&"bind"), "missing bind kind: {kinds:?}");
    assert!(kinds.contains(&"socket"), "missing socket kind: {kinds:?}");
}

#[test]
fn bind_mode_serializes_as_snake_case() {
    assert_eq!(serde_json::to_string(&BindMode::Ro).unwrap(), "\"ro\"");
    assert_eq!(serde_json::to_string(&BindMode::Rw).unwrap(), "\"rw\"");
    assert_eq!(
        serde_json::to_string(&BindMode::CreateInsideJail).unwrap(),
        "\"create_inside_jail\"",
    );
}

#[test]
fn resource_limits_pid_namespace_daemonize_and_netns_persist_in_plan_json() {
    let mut cfg = common::minimal_config(Path::new("/tmp/run/vm-limits"));
    cfg.resource_limits = m80_jailer::ResourceLimits {
        no_file: 1024,
        fsize: Some(4096),
    };
    cfg.new_pid_ns = true;
    cfg.daemonize = true;
    cfg.netns_path = Some(PathBuf::from("/var/run/netns/m80-test"));

    let plan = Plan::compute(&cfg).unwrap();
    let v: serde_json::Value = serde_json::to_value(&plan).unwrap();

    assert_eq!(v["config"]["resource_limits"]["no_file"], 1024);
    assert_eq!(v["config"]["resource_limits"]["fsize"], 4096);
    assert_eq!(v["config"]["new_pid_ns"], true);
    assert_eq!(v["config"]["daemonize"], true);
    assert_eq!(v["config"]["netns_path"], "/var/run/netns/m80-test");
}
