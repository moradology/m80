use std::path::Path;

use m80_jailer::{CgroupVersion, Plan};

#[test]
fn cgroup_version_v2_persists_in_replayable_plan_json() {
    let mut cfg = crate::common::minimal_config(Path::new("/tmp/run/vm-cgroup-v2"));
    cfg.cgroup_version = Some(CgroupVersion::V2);

    let plan = Plan::compute(&cfg).unwrap();
    let value: serde_json::Value = serde_json::to_value(&plan).unwrap();

    assert_eq!(value["config"]["cgroup_version"], "v2");
}
