use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

use m80_test_helpers::reaper::{
    is_owned_iptables_chain, is_owned_link_name, is_stale_run_dir, owned_iptables_chains,
    owned_iptables_delete_rules, owned_link_names,
};

#[test]
fn owned_link_names_accept_only_m80_shapes() {
    let links = "\
1: lo: <LOOPBACK> mtu 65536 qdisc noop state UNKNOWN mode DEFAULT group default qlen 1000
8: tfc1234abcd5678@if7: <BROADCAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default
9: brfc1234abcd567: <BROADCAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default
10: bfc1234abcd5678: <BROADCAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default
11: m80-br-legacy: <BROADCAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default
12: eth0@if2: <BROADCAST> mtu 1500 qdisc noop state UP mode DEFAULT group default
13: tfcnothexzzzzz: <BROADCAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default
";

    assert_eq!(
        owned_link_names(links),
        vec![
            "tfc1234abcd5678",
            "brfc1234abcd567",
            "bfc1234abcd5678",
            "m80-br-legacy"
        ]
    );
    assert!(is_owned_link_name("tfcabcdef123456"));
    assert!(!is_owned_link_name("tap0"));
}

#[test]
fn owned_iptables_rules_require_m80_comment() {
    let rules = r#"
-P FORWARD ACCEPT
-A FORWARD -i brfc1234abcd5 -m comment --comment "m80:abc123def456:tfc1234abcd5678" -j tfwabcdef123456
-A FORWARD -i brfc1234abcd5 -m comment --comment "foreign" -j ACCEPT
-A tfwabcdef123456 -m comment --comment "m80:abc123def456:tfc1234abcd5678" -j ACCEPT
-N tfwabcdef123456
-N user-chain
"#;

    let delete = owned_iptables_delete_rules("filter", rules);
    assert_eq!(delete.len(), 2);
    assert_eq!(delete[0].table, "filter");
    assert_eq!(delete[0].chain, "FORWARD");
    assert_eq!(delete[1].chain, "tfwabcdef123456");
    assert_eq!(
        owned_iptables_chains(rules),
        vec!["tfwabcdef123456".to_owned()]
    );
    assert!(is_owned_iptables_chain("tfwabcdef123456"));
    assert!(!is_owned_iptables_chain("FORWARD"));
}

#[test]
fn stale_run_dir_requires_age_and_no_live_pid() {
    let temp = tempfile::tempdir().unwrap();
    let stale = temp.path().join("vm-stale");
    let live = temp.path().join("vm-live");
    let warm = temp.path().join("warm");
    std::fs::create_dir(&stale).unwrap();
    std::fs::create_dir(&live).unwrap();
    std::fs::create_dir(&warm).unwrap();
    std::fs::write(stale.join("firecracker.pid"), "999999\n").unwrap();
    std::fs::write(
        live.join("firecracker.pid"),
        format!("{}\n", std::process::id()),
    )
    .unwrap();

    let now = SystemTime::now() + Duration::from_secs(7200);
    assert!(is_stale_run_dir(&stale, now, Duration::from_secs(3600)));
    assert!(!is_stale_run_dir(&live, now, Duration::from_secs(3600)));
    assert!(!is_stale_run_dir(&warm, now, Duration::from_secs(3600)));
    assert!(!is_stale_run_dir(
        &stale,
        SystemTime::now(),
        Duration::from_secs(3600)
    ));
}

#[test]
#[ignore = "requires-root requires-network-namespace"]
fn host_reaper_removes_seeded_m80_residue() {
    let suffix = format!("{:012x}", std::process::id());
    let tap = format!("tfc{suffix}");
    let bridge = format!("brfc{}", &suffix[..11]);
    let chain = format!("tfw{suffix}");
    let comment = format!("m80:reaper-test:{suffix}");
    let run_root = PathBuf::from(format!("/tank/tmp/m80-reaper-test-{suffix}"));
    let residue = HostResidue {
        tap,
        bridge,
        chain,
        comment,
        run_root,
    };
    residue.cleanup();

    assert_success(
        "create stale tap",
        privileged("ip", &["tuntap", "add", "dev", &residue.tap, "mode", "tap"]),
    );
    assert_success(
        "create stale bridge",
        privileged(
            "ip",
            &["link", "add", "name", &residue.bridge, "type", "bridge"],
        ),
    );
    assert_success(
        "create stale iptables chain",
        privileged("iptables", &["-w", "-N", &residue.chain]),
    );
    assert_success(
        "create m80-owned chain rule",
        privileged(
            "iptables",
            &[
                "-w",
                "-A",
                &residue.chain,
                "-m",
                "comment",
                "--comment",
                &residue.comment,
                "-j",
                "RETURN",
            ],
        ),
    );
    assert_success(
        "create m80-owned FORWARD rule",
        privileged(
            "iptables",
            &[
                "-w",
                "-A",
                "FORWARD",
                "-m",
                "comment",
                "--comment",
                &residue.comment,
                "-j",
                "ACCEPT",
            ],
        ),
    );

    let stale_run_dir = residue.run_root.join("vm-stale");
    std::fs::create_dir_all(&stale_run_dir).unwrap();
    std::fs::write(stale_run_dir.join("firecracker.pid"), "999999\n").unwrap();
    std::thread::sleep(Duration::from_millis(25));

    let mut reaper = Command::new(repo_root().join("scripts/e2e-reap.sh"));
    reaper.args([
        "--run-root",
        residue.run_root.to_str().unwrap(),
        "--min-age-hours",
        "0",
    ]);
    assert_success("run e2e reaper", reaper);

    assert_not_success(
        "tap removed",
        privileged("ip", &["link", "show", "dev", &residue.tap]),
    );
    assert_not_success(
        "bridge removed",
        privileged("ip", &["link", "show", "dev", &residue.bridge]),
    );
    assert_not_success(
        "iptables chain removed",
        privileged("iptables", &["-w", "-S", &residue.chain]),
    );
    assert_not_success(
        "FORWARD rule removed",
        privileged(
            "iptables",
            &[
                "-w",
                "-C",
                "FORWARD",
                "-m",
                "comment",
                "--comment",
                &residue.comment,
                "-j",
                "ACCEPT",
            ],
        ),
    );
    assert!(!stale_run_dir.exists());
}

struct HostResidue {
    tap: String,
    bridge: String,
    chain: String,
    comment: String,
    run_root: PathBuf,
}

impl HostResidue {
    fn cleanup(&self) {
        cleanup_command(privileged(
            "iptables",
            &[
                "-w",
                "-D",
                "FORWARD",
                "-m",
                "comment",
                "--comment",
                &self.comment,
                "-j",
                "ACCEPT",
            ],
        ));
        cleanup_command(privileged("iptables", &["-w", "-F", &self.chain]));
        cleanup_command(privileged("iptables", &["-w", "-X", &self.chain]));
        cleanup_command(privileged("ip", &["link", "delete", &self.tap]));
        cleanup_command(privileged("ip", &["link", "delete", &self.bridge]));
        let _ = std::fs::remove_dir_all(&self.run_root);
    }
}

impl Drop for HostResidue {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn privileged(program: &str, args: &[&str]) -> Command {
    let mut command = if is_root() {
        Command::new(program)
    } else {
        let mut command = Command::new("sudo");
        command.arg("-n").arg(program);
        command
    };
    command.args(args);
    command
}

fn is_root() -> bool {
    let output = Command::new("id").arg("-u").output().unwrap();
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "0"
}

fn assert_success(description: &str, mut command: Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{description} failed:\n{}",
        output_text(&output)
    );
}

fn assert_not_success(description: &str, mut command: Command) {
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "{description} unexpectedly succeeded:\n{}",
        output_text(&output)
    );
}

fn output_text(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn cleanup_command(mut command: Command) {
    let _ = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
}
