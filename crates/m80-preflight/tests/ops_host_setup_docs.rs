//! Regression checks for the production host setup operator guide.

const HOST_SETUP: &str = include_str!("../../../docs/ops/host-setup.md");

#[test]
fn host_setup_doc_pins_security_sections() {
    for required in [
        "## Identity Model",
        "Docker daemon access as host root",
        "## Artifact Ownership And Modes",
        "/opt/m80/artifacts",
        "host-binaries.manifest.json",
        "docs/ops/binary-installation.md",
        "## Cargo And Rust Supply Chain",
        "replace-with = \"hostile\"",
        "cargo build --locked --offline --release",
        "## Build Environment",
        "no network during the compile/package phase",
        "## Runtime Host Preconditions",
        "net.netfilter.nf_conntrack_max",
        "## Runtime Privilege",
        "cap_setpcap+ep",
        "## CI Hardening",
        "pin third-party GitHub Actions by commit SHA",
    ] {
        assert!(
            HOST_SETUP.contains(required),
            "host setup doc must contain {required:?}"
        );
    }
}
