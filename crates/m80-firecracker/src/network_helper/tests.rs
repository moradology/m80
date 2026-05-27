use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use m80_net_mode::OutboundIntent;
use m80_net_outbound::NetworkHelperFailureKind;

use super::*;

fn write_helper_script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("helper.sh");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(body.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

#[test]
fn realize_bridge_and_tap_uses_helper_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("requests.log");
    let script = format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "{}"
  printf '%s\n' '{{"status":"ok","success":{{"kind":"realized_network","realized":{{"bridge_name":"br-test","tap_name":"tap-test","vmm_netns_path":"/run/netns/m80-test","guest_ipv4":"172.16.0.2","guest_mac":"02:00:00:00:00:01","bridge_cidr":"172.16.0.0/24"}}}}}}'
done
"#,
        log.display()
    );
    let helper = NetworkHelperClient::new(write_helper_script(dir.path(), &script));

    let realized = helper
        .realize_bridge_and_tap(
            OutboundIntent {
                exceptions: Vec::new(),
            },
            "vm-a",
            Path::new("/run/m80"),
            Path::new("/run/m80/vm-a"),
        )
        .unwrap();

    assert_eq!(realized.tap_name, "tap-test");
    let requests = std::fs::read_to_string(log).unwrap();
    assert!(requests.contains(r#""op":"realize_bridge_and_tap""#));
    assert!(requests.contains(r#""vm_id":"vm-a""#));
}

#[test]
fn helper_operation_failure_stays_typed() {
    let dir = tempfile::tempdir().unwrap();
    let script = r#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"err","failure":{"kind":"operation_failed","detail":"synthetic helper denial"}}'
done
"#;
    let helper = NetworkHelperClient::new(write_helper_script(dir.path(), script));

    let err = helper
        .cleanup_vm("vm-a", Path::new("/run/m80"))
        .unwrap_err();

    assert!(matches!(
        err,
        NetworkHelperError::OperationFailed {
            operation: NetworkHelperOperation::CleanupVm,
            kind: NetworkHelperFailureKind::OperationFailed,
            ref detail,
        } if detail == "synthetic helper denial"
    ));
}

#[test]
fn oversized_helper_response_is_rejected_before_decode() {
    let mut input = vec![b' '; NETWORK_HELPER_MAX_FRAME_BYTES + 1];
    input.push(b'\n');

    let err = read_response(NetworkHelperOperation::CleanupVm, &mut &input[..]).unwrap_err();

    assert!(matches!(
        err,
        NetworkHelperError::OversizedResponse {
            operation: NetworkHelperOperation::CleanupVm,
            limit: NETWORK_HELPER_MAX_FRAME_BYTES,
        }
    ));
}

#[test]
fn process_global_helper_rejects_launch_mode_switch_for_same_binary() {
    let dir = tempfile::tempdir().unwrap();
    let helper_bin = write_helper_script(
        dir.path(),
        r#"#!/bin/sh
while IFS= read -r _line; do
  printf '%s\n' '{"status":"ok","success":{"kind":"empty"}}'
done
"#,
    );
    let mut slot = None;
    let helper = backend_network_helper_from_slot(
        &mut slot,
        NetworkHelperLaunch::direct(helper_bin.clone()),
    )
    .unwrap();

    let err = backend_network_helper_from_slot(
        &mut slot,
        NetworkHelperLaunch::Systemd {
            systemd_run_bin: PathBuf::from("/usr/bin/systemd-run"),
            helper_bin,
        },
    )
    .unwrap_err();

    assert!(matches!(
        err,
        NetworkHelperError::LaunchConfigMismatch {
            ref active,
            ref requested,
        } if active.starts_with("direct:") && requested.starts_with("systemd:/usr/bin/systemd-run:")
    ));
    drop(helper);
}

#[test]
#[ignore = "requires-root requires-systemd"]
fn systemd_helper_process_state_is_pinned() {
    if !nix::unistd::Uid::effective().is_root() {
        eprintln!("skipping: run as root to inspect the systemd helper envelope");
        return;
    }
    let systemd_run_bin = std::env::var_os("M80_SYSTEMD_RUN_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/systemd-run"));
    if !systemd_run_bin.exists() {
        eprintln!("skipping: {} does not exist", systemd_run_bin.display());
        return;
    }
    let ip_bin = Path::new("/usr/sbin/ip");
    if !ip_bin.exists() {
        eprintln!("skipping: {} does not exist", ip_bin.display());
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let status_path = dir.path().join("status");
    let netns_name = format!("m80test{:x}", std::process::id());
    let netns_guard = NetnsGuard {
        ip_bin: ip_bin.to_path_buf(),
        name: netns_name.clone(),
    };
    netns_guard.delete();
    let script = format!(
        r#"#!/bin/sh
set -eu
cat /proc/self/status > "{}"
if IFS= read -r _line; then
  /usr/sbin/ip netns add "{}"
  printf '%s\n' '{{"status":"ok","success":{{"kind":"empty"}}}}'
fi
"#,
        status_path.display(),
        netns_name
    );
    let helper = NetworkHelperClient::with_launch(NetworkHelperLaunch::Systemd {
        systemd_run_bin,
        helper_bin: write_helper_script(dir.path(), &script),
    });

    helper.cleanup_vm("vm-a", Path::new("/run/m80")).unwrap();

    let status = std::fs::read_to_string(&status_path).unwrap();
    let expected = cap_mask(12) | cap_mask(21);
    assert_eq!(status_hex_value(&status, "CapBnd"), expected);
    assert_eq!(status_hex_value(&status, "CapAmb"), expected);
    assert_eq!(status_hex_value(&status, "CapEff"), expected);
    assert_eq!(status_decimal_value(&status, "NoNewPrivs"), 1);

    let join = Command::new(ip_bin)
        .args(["netns", "exec", &netns_name, "true"])
        .output()
        .unwrap();
    assert!(
        join.status.success(),
        "host could not join netns created by systemd helper: status={}; stderr={}",
        join.status,
        String::from_utf8_lossy(&join.stderr)
    );
}

struct NetnsGuard {
    ip_bin: PathBuf,
    name: String,
}

impl NetnsGuard {
    fn delete(&self) {
        let _ = Command::new(&self.ip_bin)
            .args(["netns", "del", &self.name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl Drop for NetnsGuard {
    fn drop(&mut self) {
        self.delete();
    }
}

fn cap_mask(bit: u32) -> u64 {
    1u64 << bit
}

fn status_hex_value(status: &str, field: &str) -> u64 {
    let raw = status_field(status, field);
    u64::from_str_radix(raw, 16).unwrap_or_else(|_| panic!("{field} was not hex: {raw:?}"))
}

fn status_decimal_value(status: &str, field: &str) -> u64 {
    let raw = status_field(status, field);
    raw.parse()
        .unwrap_or_else(|_| panic!("{field} was not decimal: {raw:?}"))
}

fn status_field<'a>(status: &'a str, field: &str) -> &'a str {
    let prefix = format!("{field}:\t");
    status
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .unwrap_or_else(|| panic!("{field} missing from /proc status"))
}
