# Firecracker /dev/kvm Residual Surface

m80 documents `/dev/kvm` as residual Firecracker runtime attack surface after
`InstanceStart`. The official jailer must expose `/dev/kvm` to Firecracker, and
Firecracker keeps KVM file descriptors open while the VM runs.

m80 does not install a phase-two seccomp filter from outside Firecracker. A
post-startup policy that narrows KVM ioctls needs supported Firecracker
functionality or a deliberately owned custom Firecracker build.

The decision record is
`docs/decisions/0005-firecracker-dev-kvm-residual-surface.md`.

Tests:

- `crates/m80-firecracker/tests/seccomp_deferred_docs.rs::dev_kvm_residual_surface_docs_pin_deferred_boundary`
