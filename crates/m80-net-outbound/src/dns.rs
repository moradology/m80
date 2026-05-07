use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;

use crate::NetError;

/// Output returned by DNS discovery helper commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsCommandOutput {
    /// Whether the command exited successfully.
    pub status_success: bool,
    /// Captured stdout as best-effort UTF-8.
    pub stdout: String,
    /// Captured stderr as best-effort UTF-8.
    pub stderr: String,
}

impl DnsCommandOutput {
    /// Construct a successful DNS command output with stdout.
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            status_success: true,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// Construct a failed DNS command output with stderr.
    pub fn failure(stderr: impl Into<String>) -> Self {
        Self {
            status_success: false,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }
}

/// Host seam for DNS resolver discovery.
pub trait DnsDiscoveryOps {
    /// Run a helper command and return stdout/stderr without interpreting exit status.
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<DnsCommandOutput, NetError>;

    /// Read a host text file.
    fn read_to_string(&mut self, path: &Path) -> Result<String, NetError>;
}

/// Real-host backend for DNS resolver discovery via shell commands and `/etc/resolv.conf`.
pub struct CommandDnsDiscoveryOps;

impl DnsDiscoveryOps for CommandDnsDiscoveryOps {
    fn command_output(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<DnsCommandOutput, NetError> {
        let output = Command::new(program).args(args).output()?;
        Ok(DnsCommandOutput {
            status_success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn read_to_string(&mut self, path: &Path) -> Result<String, NetError> {
        Ok(std::fs::read_to_string(path)?)
    }
}

/// Discover admitted DNS resolvers through a supplied host seam.
pub fn discover_dns_resolvers_with_ops(
    ops: &mut impl DnsDiscoveryOps,
) -> Result<Vec<Ipv4Addr>, NetError> {
    if let Ok(output) = ops.command_output("resolvectl", &["dns".to_owned()]) {
        if output.status_success {
            let resolvers = parse_resolver_tokens(&output.stdout);
            if !resolvers.is_empty() {
                return Ok(resolvers);
            }
        }
    }

    let resolv_conf = ops.read_to_string(Path::new("/etc/resolv.conf"))?;
    let resolvers = parse_resolv_conf(&resolv_conf);
    if resolvers.is_empty() {
        Err(NetError::NoUsableDnsResolvers)
    } else {
        Ok(resolvers)
    }
}

/// Return true when an IPv4 address is admissible as an upstream DNS resolver.
pub fn is_admitted_dns_resolver(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || address.is_documentation()
    {
        return false;
    }
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return false;
    }
    if octets[0] == 198 && matches!(octets[1], 18 | 19) {
        return false;
    }
    if octets[0] == 0 || octets[0] >= 224 {
        return false;
    }
    true
}

fn parse_resolv_conf(text: &str) -> Vec<Ipv4Addr> {
    let mut resolvers = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.first() == Some(&"nameserver") {
            if let Some(candidate) = fields.get(1) {
                push_admitted_resolver(&mut resolvers, candidate);
            }
        }
    }
    resolvers
}

fn parse_resolver_tokens(text: &str) -> Vec<Ipv4Addr> {
    let mut resolvers = Vec::new();
    for token in text.split_whitespace() {
        let candidate =
            token.trim_matches(|ch: char| matches!(ch, ',' | ';' | '[' | ']' | '(' | ')'));
        push_admitted_resolver(&mut resolvers, candidate);
    }
    resolvers
}

fn push_admitted_resolver(resolvers: &mut Vec<Ipv4Addr>, candidate: &str) {
    let Ok(address) = candidate.parse::<Ipv4Addr>() else {
        return;
    };
    if is_admitted_dns_resolver(address) && !resolvers.contains(&address) {
        resolvers.push(address);
    }
}
