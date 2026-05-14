const ADR: &str =
    include_str!("../../../docs/decisions/0005-firecracker-dev-kvm-residual-surface.md");
const BEHAVIOR: &str =
    include_str!("../../../docs/behaviors/security/firecracker-dev-kvm-residual-surface.md");

#[test]
fn dev_kvm_residual_surface_docs_pin_deferred_boundary() {
    assert!(ADR.contains("Accepted as a documented deferred gap"));
    assert!(ADR.contains("m80 will not attempt an LD_PRELOAD, ptrace, or wrapper-based"));
    assert!(ADR.contains("upstream Firecracker feature"));
    assert!(ADR.contains("post-`InstanceStart`"));

    assert!(BEHAVIOR.contains("does not install a phase-two seccomp filter"));
    assert!(BEHAVIOR.contains("0005-firecracker-dev-kvm-residual-surface.md"));
}
