use clap::Parser;
use m80_cli::{Cli, Cmd, NetAction};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::PermissionsExt as _;

use assert_cmd::Command;

#[test]
fn net_cleanup_dry_run_cli_shape_is_registered() {
    let cli = Cli::try_parse_from(["m80", "net", "cleanup", "--dry-run"]).unwrap();
    let Cmd::Net {
        action: NetAction::Cleanup(args),
    } = cli.subcommand
    else {
        panic!("expected net cleanup subcommand");
    };
    assert!(args.dry_run);
}

#[test]
fn net_cleanup_uses_tag_scan_and_delete_commands() {
    let temp = tempfile::TempDir::new().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let run_root = temp.path().join("run-root");
    fs::create_dir_all(&run_root).unwrap();
    let filter_rules = temp.path().join("filter.rules");
    let nat_rules = temp.path().join("nat.rules");
    let links = temp.path().join("links.txt");
    let log = temp.path().join("commands.log");

    let tap = "tfc0123456789ab";
    let chain = "tfw0123456789ab";
    let digest = run_root_digest_prefix(&run_root);
    fs::write(
        &filter_rules,
        format!("-A FORWARD -m comment --comment m80:{digest}:{tap} -j {chain}\n"),
    )
    .unwrap();
    fs::write(&nat_rules, "").unwrap();
    fs::write(
        &links,
        format!("7: {tap}: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN\n"),
    )
    .unwrap();
    write_executable(
        &bin_dir.join("iptables"),
        r#"#!/bin/sh
printf 'iptables %s\n' "$*" >> "$M80_FAKE_NET_LOG"
if [ "$1" = "-w" ] && [ "$2" = "-t" ] && [ "$4" = "-S" ]; then
  if [ "$3" = "filter" ]; then
    cat "$M80_FAKE_FILTER"
  else
    cat "$M80_FAKE_NAT"
  fi
fi
exit 0
"#,
    );
    write_executable(
        &bin_dir.join("ip"),
        r#"#!/bin/sh
printf 'ip %s\n' "$*" >> "$M80_FAKE_NET_LOG"
if [ "$1" = "-o" ] && [ "$2" = "link" ] && [ "$3" = "show" ]; then
  cat "$M80_FAKE_LINKS"
fi
exit 0
"#,
    );

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::cargo_bin("m80")
        .unwrap()
        .env("PATH", path)
        .env("M80_RUN_ROOT", &run_root)
        .env("M80_FAKE_FILTER", &filter_rules)
        .env("M80_FAKE_NAT", &nat_rules)
        .env("M80_FAKE_LINKS", &links)
        .env("M80_FAKE_NET_LOG", &log)
        .args(["--json", "net", "cleanup"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(parsed["data"]["resources"][0]["action"], "removed");
    assert_eq!(parsed["data"]["resources"][0]["resource_kind"], "iptables");
    assert_eq!(parsed["data"]["resources"][2]["resource_kind"], "tap");
    assert_eq!(parsed["data"]["resources"][2]["action"], "removed");

    let log = fs::read_to_string(log).unwrap();
    assert!(log.contains("iptables -w -t filter -D FORWARD"));
    assert!(log.contains("iptables -w -t filter -X tfw0123456789ab"));
    assert!(log.contains("ip link delete tfc0123456789ab"));
}

fn write_executable(path: &std::path::Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn run_root_digest_prefix(run_root: &std::path::Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(run_root.as_os_str().as_bytes());
    first_hex_chars(&hasher.finalize(), 12)
}

fn first_hex_chars(bytes: &[u8], chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(chars);
    for byte in bytes {
        if out.len() == chars {
            break;
        }
        out.push(HEX[(byte >> 4) as usize] as char);
        if out.len() == chars {
            break;
        }
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
