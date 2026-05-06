use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use m80_net_outbound::{
    inject_guest_network_config_with_ops, planned_bridge_state, planned_vm_network_state,
    read_vm_network_state_record, DnsCommandOutput, DnsDiscoveryOps, GuestNetworkConfigOps,
    NetError, OutboundIntent, SetupPhase, VmNetworkStateRecord, M80_NETWORKD_FILE,
    M80_RESOLVED_FILE,
};

#[test]
fn networkd_unit_injected_with_static_ip_and_dns() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_rootfs = temp.path().join("rootfs.overlay.ext4");
    let mut state = ready_state(temp.path());
    let mut ops = FakeGuestConfigOps::with_resolvectl("Global: 1.1.1.1 8.8.8.8\n");

    inject_guest_network_config_with_ops(&mut ops, &mut state, &runtime_rootfs).unwrap();

    let networkd = ops.write_content(M80_NETWORKD_FILE);
    assert_eq!(
        networkd,
        format!(
            "[Match]\nMACAddress={}\n\n[Network]\nAddress={}/{}\nGateway={}\nDNS=1.1.1.1\nDNS=8.8.8.8\nIPv6AcceptRA=no\nLinkLocalAddressing=no\n",
            state.guest_mac,
            state.guest_ipv4,
            state.bridge.cidr.prefix_len(),
            state.bridge.gateway_ipv4,
        )
    );
    assert!(ops.mkdir_commands().contains(&format!(
        "debugfs -w -R mkdir /etc/systemd/network {}",
        runtime_rootfs.display()
    )));
    assert_eq!(
        state.dns_resolvers,
        [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)]
    );
    assert!(state.runtime_rootfs_configured);
    assert_eq!(
        read_vm_network_state_record(&state.run_dir)
            .unwrap()
            .runtime_rootfs_configured,
        true
    );
}

#[test]
fn resolved_dropin_injected_with_admitted_dns() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_rootfs = temp.path().join("rootfs.overlay.ext4");
    let mut state = ready_state(temp.path());
    let mut ops = FakeGuestConfigOps::with_resolvectl("Global: 9.9.9.9 10.0.0.1 1.1.1.1\n");

    inject_guest_network_config_with_ops(&mut ops, &mut state, &runtime_rootfs).unwrap();

    assert_eq!(
        ops.write_content(M80_RESOLVED_FILE),
        "[Resolve]\nDNS=9.9.9.9 1.1.1.1\nFallbackDNS=\nDomains=~.\n"
    );
    assert!(ops.mkdir_commands().contains(&format!(
        "debugfs -w -R mkdir /etc/systemd/resolved.conf.d {}",
        runtime_rootfs.display()
    )));
}

#[test]
fn guest_daemon_does_not_touch_networking() {
    let guestd_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("m80-guestd")
        .join("src");

    for path in rust_files(&guestd_src) {
        let text = std::fs::read_to_string(&path).unwrap();
        for forbidden in [
            "/etc/systemd/network",
            "/etc/resolv.conf",
            "resolvectl",
            "systemd-networkd",
            "10-m80-outbound",
            "10-m80-dns",
            "ip addr",
            "ip link",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} must not contain guest networking write path {forbidden:?}",
                path.display()
            );
        }
    }
}

fn ready_state(run_root: &Path) -> VmNetworkStateRecord {
    let run_dir = run_root.join("vm-a");
    std::fs::create_dir(&run_dir).unwrap();
    let intent = OutboundIntent {
        exceptions: Vec::new(),
        gateway_override: None,
    };
    let bridge = planned_bridge_state(run_root, &intent)
        .unwrap()
        .with_phase(SetupPhase::Ready);
    let mut state = planned_vm_network_state(&intent, "vm-a", run_root, &run_dir, bridge);
    state.setup_phase = SetupPhase::Ready;
    state
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path)
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}

struct FakeGuestConfigOps {
    resolvectl: DnsCommandOutput,
    resolv_conf: String,
    commands: Vec<String>,
    writes: Vec<(String, String)>,
}

impl FakeGuestConfigOps {
    fn with_resolvectl(stdout: &str) -> Self {
        Self {
            resolvectl: DnsCommandOutput::success(stdout),
            resolv_conf: String::new(),
            commands: Vec::new(),
            writes: Vec::new(),
        }
    }

    fn write_content(&self, image_path: &str) -> &str {
        self.writes
            .iter()
            .find(|(candidate, _)| candidate == image_path)
            .map(|(_, content)| content.as_str())
            .unwrap_or_else(|| panic!("missing write for {image_path}"))
    }

    fn mkdir_commands(&self) -> Vec<String> {
        self.commands
            .iter()
            .filter(|command| command.contains(" -R mkdir "))
            .cloned()
            .collect()
    }
}

impl DnsDiscoveryOps for FakeGuestConfigOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<DnsCommandOutput, NetError> {
        self.commands.push(format!("{program} {}", args.join(" ")));
        match program {
            "resolvectl" => Ok(self.resolvectl.clone()),
            "debugfs" => Ok(DnsCommandOutput::failure("not found")),
            other => panic!("unexpected command_output program {other}"),
        }
    }

    fn read_to_string(&mut self, _path: &Path) -> Result<String, NetError> {
        Ok(self.resolv_conf.clone())
    }
}

impl GuestNetworkConfigOps for FakeGuestConfigOps {
    fn run_command(&mut self, program: &str, args: &[String]) -> Result<(), NetError> {
        self.commands.push(format!("{program} {}", args.join(" ")));
        if program == "debugfs" && args[1] == "-R" && args[2].starts_with("write ") {
            let mut fields = args[2].split_whitespace();
            assert_eq!(fields.next(), Some("write"));
            let temp_path = fields.next().unwrap();
            let image_path = fields.next().unwrap();
            self.writes.push((
                image_path.to_owned(),
                std::fs::read_to_string(temp_path).unwrap(),
            ));
        }
        Ok(())
    }
}
