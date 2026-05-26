use std::path::PathBuf;

use m80_test_helpers::leak_check::LeakSnapshot;

#[test]
fn unchanged_owned_resources_are_not_leaks() {
    let ip = "8: tfc1234abcd5678@if7: <BROADCAST> mtu 1500 qdisc noop state DOWN\n";
    let filter = r#"
-N tfwabcdef123456
-A FORWARD -m comment --comment "m80:vm:tfc1234abcd5678" -j ACCEPT
"#;
    let before = LeakSnapshot::from_observations(
        ip,
        filter,
        "",
        [PathBuf::from("/tank/tmp/m80-run/vm-a")],
        [PathBuf::from("/sys/fs/cgroup/m80-firecracker/vm-a")],
    );
    let after = before.clone();

    assert!(before.diff_new_resources(&after).is_clean());
}

#[test]
fn foreign_resources_are_ignored() {
    let after = LeakSnapshot::from_observations(
        "1: eth0@if2: <BROADCAST> mtu 1500 qdisc noop state UP\n",
        r#"
-N user-chain
-A FORWARD -m comment --comment "foreign" -j ACCEPT
"#,
        "",
        [PathBuf::from("/tank/tmp/m80-run/.preserved")],
        [],
    );

    assert!(LeakSnapshot::default()
        .diff_new_resources(&after)
        .is_clean());
}

#[test]
fn deliberately_leaky_snapshot_fails_check() {
    let after = LeakSnapshot::from_observations(
        "8: tfc1234abcd5678@if7: <BROADCAST> mtu 1500 qdisc noop state DOWN\n",
        r#"
-N tfwabcdef123456
-A FORWARD -m comment --comment "m80:vm:tfc1234abcd5678" -j ACCEPT
"#,
        r#"
-A POSTROUTING -m comment --comment "m80:vm:tfc1234abcd5678" -j MASQUERADE
"#,
        [PathBuf::from("/tank/tmp/m80-run/vm-leaked")],
        [PathBuf::from("/sys/fs/cgroup/m80-firecracker/vm-leaked")],
    );

    let report = LeakSnapshot::default().diff_new_resources(&after);
    let rendered = report.to_string();
    assert!(!report.is_clean());
    assert!(rendered.contains("link: tfc1234abcd5678"));
    assert!(rendered.contains("iptables-chain: filter/tfwabcdef123456"));
    assert!(rendered.contains("iptables-rule: filter/FORWARD"));
    assert!(rendered.contains("iptables-rule: nat/POSTROUTING"));
    assert!(rendered.contains("run-dir: /tank/tmp/m80-run/vm-leaked"));
    assert!(rendered.contains("cgroup: /sys/fs/cgroup/m80-firecracker/vm-leaked"));
}
