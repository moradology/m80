# Layer-1 Firecracker Configuration Audit

Layer 1 is the Firecracker/KVM isolation boundary. m80 does not implement the
hypervisor, but it owns the Firecracker configuration it sends before
`InstanceStart`.

Pinned configuration:

- `PUT /machine-config` includes `smt = false`.
- By default, `PUT /machine-config` omits `cpu_template`. This is the
  latency-first same-host shape. Callers that need AWS template masking for
  cross-host snapshot portability opt in through `SandboxConfig::cpu_template`.
- The preboot PUT plan contains only documented devices: machine config, boot
  source, rootfs drive, rootfs overlay drive, optional workspace drive,
  optional preallocated hotplug drive slots, optional outbound NAT NIC, and
  vsock.
- Boot source has no initrd.
- Firecracker API and vsock sockets live under the official jailer chroot path,
  not directly under the host run directory.
- m80 exposes no Firecracker client endpoint for IOMMU passthrough, debug-reg
  capabilities, MSR allowlist mutation, or arbitrary virtio device injection.

Verification:

- `crates/m80-firecracker/src/preboot.rs::tests::layer_1_machine_config_omits_cpu_template_by_default`
- `crates/m80-firecracker/src/preboot.rs::tests::machine_config_honors_explicit_cpu_template`
- `crates/m80-firecracker/src/preboot.rs::tests::layer_1_preboot_plan_contains_only_documented_devices`
- `crates/m80-firecracker-client/tests/put_each_resource.rs::put_machine_config_sends_correct_json`
- `crates/m80-firecracker/tests/lifecycle/run_dir_layout.rs::api_socket_path_is_inside_jailer_root`
