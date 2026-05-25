use super::host::{HostCommandOutput, HostCommands};
use super::*;
use tempfile::TempDir;

#[derive(Default)]
struct FakeHostCommands {
    filter: String,
    nat: String,
    links: String,
    runs: Vec<String>,
}

impl HostCommands for FakeHostCommands {
    fn output(&mut self, program: &str, args: &[&str]) -> Result<HostCommandOutput, FcError> {
        let stdout = match (program, args) {
            ("iptables", ["-w", "-t", "filter", "-S"]) => self.filter.clone(),
            ("iptables", ["-w", "-t", "nat", "-S"]) => self.nat.clone(),
            ("ip", ["-o", "link", "show"]) => self.links.clone(),
            _ => {
                self.runs.push(format!("{program} {}", args.join(" ")));
                String::new()
            }
        };
        Ok(HostCommandOutput {
            status_success: true,
            stdout,
            stderr: String::new(),
        })
    }

    fn run(&mut self, program: &str, args: &[&str]) -> Result<(), FcError> {
        self.runs.push(format!("{program} {}", args.join(" ")));
        Ok(())
    }
}

#[test]
fn dry_run_reports_orphan_rule_and_tap_without_deleting() {
    let root = TempDir::new().unwrap();
    let digest = run_root_digest_prefix(root.path());
    let tap = "tfc0123456789ab";
    let mut commands = FakeHostCommands {
        filter: format!(
            "-A FORWARD -m comment --comment {RULE_COMMENT_PREFIX}:{digest}:{tap} -j tfw0123456789ab\n"
        ),
        links: format!("7: {tap}: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default\n"),
        ..FakeHostCommands::default()
    };

    let report = build_net_cleanup_report(root.path(), true, &mut commands).unwrap();

    assert_eq!(report.resources.len(), 3);
    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "iptables"
            && row.action == CleanupAction::WouldRemove
            && row.orphan_vm_id == UNKNOWN_VM_ID
    }));
    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "tap"
            && row.action == CleanupAction::WouldRemove
            && row.resource_id == tap
    }));
    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "iptables_chain"
            && row.action == CleanupAction::WouldRemove
            && row.rule_text == "-X tfw0123456789ab"
    }));
    assert!(commands.runs.is_empty());
}

#[test]
fn cleanup_removes_orphans_but_keeps_state_backed_taps() {
    let root = TempDir::new().unwrap();
    let live_dir = root.path().join("vm-live");
    fs::create_dir_all(&live_dir).unwrap();
    let live_tap = derive_tap_name(root.path(), "vm-live");
    fs::write(
        live_dir.join(NETWORK_STATE_FILE),
        serde_json::json!({
            "vm_id": "vm-live",
            "tap_name": live_tap,
        })
        .to_string(),
    )
    .unwrap();
    let orphan_tap = "tfcfedcba987654";
    let digest = run_root_digest_prefix(root.path());
    let mut commands = FakeHostCommands {
        filter: format!(
            "-A FORWARD -m comment --comment {RULE_COMMENT_PREFIX}:{digest}:{orphan_tap} -j tfwfedcba987654\n\
             -A FORWARD -m comment --comment {RULE_COMMENT_PREFIX}:{digest}:{live_tap} -j tfw111111111111\n"
        ),
        links: format!(
            "7: {orphan_tap}: <BROADCAST> mtu 1500\n8: {live_tap}: <BROADCAST> mtu 1500\n"
        ),
        ..FakeHostCommands::default()
    };

    let report = build_net_cleanup_report(root.path(), false, &mut commands).unwrap();

    assert!(report.resources.iter().any(|row| {
        row.resource_id == "filter:FORWARD"
            && row.rule_text.contains(orphan_tap)
            && row.action == CleanupAction::Removed
    }));
    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "tap"
            && row.resource_id == live_tap
            && row.action == CleanupAction::Kept
    }));
    assert!(commands
        .runs
        .iter()
        .any(|run| run.contains("iptables -w -t filter -D FORWARD")));
    assert!(commands
        .runs
        .iter()
        .any(|run| run == "ip link delete tfcfedcba987654"));
}

#[test]
fn cleanup_removes_orphan_filter_chain_even_when_jump_is_already_gone() {
    let root = TempDir::new().unwrap();
    let digest = run_root_digest_prefix(root.path());
    let tap = "tfc0123456789ab";
    let mut commands = FakeHostCommands {
        filter: format!(
            "-A tfw0123456789ab -m comment --comment {RULE_COMMENT_PREFIX}:{digest}:{tap} -j ACCEPT\n"
        ),
        ..FakeHostCommands::default()
    };

    let report = build_net_cleanup_report(root.path(), false, &mut commands).unwrap();

    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "iptables"
            && row.resource_id == "filter:tfw0123456789ab"
            && row.action == CleanupAction::Removed
    }));
    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "iptables_chain"
            && row.resource_id == "filter:tfw0123456789ab"
            && row.action == CleanupAction::Removed
    }));
    assert!(commands
        .runs
        .iter()
        .any(|run| run.contains("iptables -w -t filter -D tfw0123456789ab")));
    assert!(commands
        .runs
        .iter()
        .any(|run| run == "iptables -w -t filter -X tfw0123456789ab"));
}

#[test]
fn cleanup_reports_unattributed_taps_without_deleting_them() {
    let root = TempDir::new().unwrap();
    let tap = "tfcaaaaaaaaaaaa";
    let mut commands = FakeHostCommands {
        links: format!("7: {tap}: <BROADCAST> mtu 1500\n"),
        ..FakeHostCommands::default()
    };

    let report = build_net_cleanup_report(root.path(), false, &mut commands).unwrap();

    assert!(report.resources.iter().any(|row| {
        row.resource_kind == "tap"
            && row.resource_id == tap
            && row.orphan_vm_id == UNKNOWN_VM_ID
            && row.action == CleanupAction::Kept
    }));
    assert!(commands.runs.is_empty());
}

#[test]
fn parser_accepts_ip_link_show_shapes() {
    assert_eq!(
        parse_tap_link("9: tfcabcdef123456@if2: <BROADCAST> mtu 1500")
            .unwrap()
            .name,
        "tfcabcdef123456"
    );
    assert_eq!(
        parse_tap_link("tfcabcdef123456: flags").unwrap().name,
        "tfcabcdef123456"
    );
    assert!(parse_tap_link("9: eth0: <BROADCAST> mtu 1500").is_none());
}
