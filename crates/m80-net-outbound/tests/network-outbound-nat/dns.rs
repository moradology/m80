use std::net::Ipv4Addr;
use std::path::Path;

use m80_net_outbound::{
    discover_dns_resolvers_with_ops, is_admitted_dns_resolver, DnsCommandOutput, DnsDiscoveryOps,
    NetError,
};

#[test]
fn resolvectl_then_resolv_conf_fallback() {
    let mut ops = FakeDnsOps {
        resolvectl: DnsCommandOutput::success("Link 2: 192.168.1.1\n"),
        resolv_conf: "nameserver 9.9.9.9\nnameserver 10.0.0.1\n".to_owned(),
        ..FakeDnsOps::default()
    };

    let resolvers = discover_dns_resolvers_with_ops(&mut ops).unwrap();

    assert_eq!(resolvers, [Ipv4Addr::new(9, 9, 9, 9)]);
    assert_eq!(ops.commands, ["resolvectl dns"]);
    assert_eq!(ops.reads, ["/etc/resolv.conf"]);
}

#[test]
fn resolvectl_admitted_resolvers_win_without_resolv_conf() {
    let mut ops = FakeDnsOps {
        resolvectl: DnsCommandOutput::success("Global: 1.1.1.1 192.168.1.1 [8.8.8.8], 1.1.1.1"),
        resolv_conf: "nameserver 9.9.9.9\n".to_owned(),
        ..FakeDnsOps::default()
    };

    let resolvers = discover_dns_resolvers_with_ops(&mut ops).unwrap();

    assert_eq!(
        resolvers,
        [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)]
    );
    assert!(ops.reads.is_empty());
}

#[test]
fn no_usable_dns_resolvers_errors() {
    let mut ops = FakeDnsOps {
        resolvectl: DnsCommandOutput::failure("resolvectl failed"),
        resolv_conf: "nameserver 10.0.0.1\nnameserver 127.0.0.1\n".to_owned(),
        ..FakeDnsOps::default()
    };

    let err = discover_dns_resolvers_with_ops(&mut ops).unwrap_err();

    assert!(matches!(err, NetError::NoUsableDnsResolvers));
}

#[test]
fn is_admitted_dns_resolver_admits_public_ipv4_only() {
    assert!(is_admitted_dns_resolver(Ipv4Addr::new(1, 1, 1, 1)));
    assert!(is_admitted_dns_resolver(Ipv4Addr::new(8, 8, 8, 8)));

    for rejected in [
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(127, 0, 0, 1),
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::new(172, 16, 0, 1),
        Ipv4Addr::new(192, 168, 0, 1),
        Ipv4Addr::new(169, 254, 1, 1),
        Ipv4Addr::new(224, 0, 0, 1),
        Ipv4Addr::new(255, 255, 255, 255),
        Ipv4Addr::new(192, 0, 2, 1),
        Ipv4Addr::new(198, 51, 100, 1),
        Ipv4Addr::new(203, 0, 113, 1),
    ] {
        assert!(
            !is_admitted_dns_resolver(rejected),
            "{rejected} must be rejected"
        );
    }
}

#[test]
fn reject_cgn_benchmark_reserved_resolvers() {
    for rejected in [
        Ipv4Addr::new(100, 64, 0, 1),
        Ipv4Addr::new(100, 127, 255, 254),
        Ipv4Addr::new(198, 18, 0, 1),
        Ipv4Addr::new(198, 19, 255, 254),
        Ipv4Addr::new(0, 1, 2, 3),
        Ipv4Addr::new(224, 0, 0, 1),
        Ipv4Addr::new(240, 0, 0, 1),
    ] {
        assert!(
            !is_admitted_dns_resolver(rejected),
            "{rejected} must be rejected"
        );
    }
}

struct FakeDnsOps {
    resolvectl: DnsCommandOutput,
    resolv_conf: String,
    commands: Vec<String>,
    reads: Vec<String>,
}

impl Default for FakeDnsOps {
    fn default() -> Self {
        Self {
            resolvectl: DnsCommandOutput::failure("not configured"),
            resolv_conf: String::new(),
            commands: Vec::new(),
            reads: Vec::new(),
        }
    }
}

impl DnsDiscoveryOps for FakeDnsOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<DnsCommandOutput, NetError> {
        self.commands.push(format!("{program} {}", args.join(" ")));
        assert_eq!(program, "resolvectl");
        Ok(self.resolvectl.clone())
    }

    fn read_to_string(&mut self, path: &Path) -> Result<String, NetError> {
        self.reads.push(path.display().to_string());
        Ok(self.resolv_conf.clone())
    }
}
