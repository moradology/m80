# In-Guest Layer 1 CVE Smoke

Layer 1 is the KVM plus Firecracker isolation boundary. m80 does not implement
that boundary, but it owns the configuration and image shape used to boot the
guest. The Layer 1 smoke test boots a normal m80 guest and runs a deliberately
curious workload inside the guest so regressions are observed at the same
boundary a caller relies on.

`crates/m80-firecracker/tests/layer1_guest_smoke_real_kvm.rs` compiles
`tests/fixtures/layer1_probe.c` into a static helper, uploads it through the
normal guest file-op channel, and executes each probe with
`RunningSandbox::exec`. The helper exits `0` only when the forbidden primitive
worked. Expected kernel or device denials print `BLOCKED <probe> ...` and exit
non-zero.

Covered probes:

- `dev_mem_nonroot`: drops to uid/gid 65534 and opens `/dev/mem`; expected
  result is `EACCES`, `EPERM`, or no device node in the stripped guest.
- `virtio_config_write`: locates a virtio sysfs `config` file and attempts a
  write with an invalid userspace buffer; expected result is `EFAULT`, `EBADF`,
  `EINVAL`, `EPERM`, `EACCES`, `EROFS`, `ENODEV`, or no exposed config file.
- `rdmsr_host_msr`: executes `rdmsr` for a host MSR from userspace; expected
  result is a trapped privileged instruction signal.
- `write_cr0` and `write_cr4`: attempt control-register writes from userspace;
  expected result is a trapped privileged instruction signal.
- `proc_kcore_nonroot`: drops to uid/gid 65534 and opens `/proc/kcore`; expected
  result is `EACCES`, `EPERM`, or `ENOENT`. The stripped m80 kernel disables
  `CONFIG_PROC_KCORE`, so absence is a valid closed surface.
- `ioperm_iopl_nonroot`: drops privileges and calls `ioperm(2)` and `iopl(2)`;
  expected result is `EPERM`.
- `vsock_non_allowed_cid`: attempts a guest-initiated vsock connection to CID
  99; expected result is refusal, no route/device, address unavailability, or a
  bounded timeout rather than a completed connection.

The test uses `RunDirDumpGuard`, so failures print the guest console and
`diagnostics.jsonl`. If one or more probes fail after the VM boots, the harness
stops the VM, preserves the run directory for triage, and reports each named
probe separately.

Verification:

- `crates/m80-firecracker/tests/layer1_guest_smoke_real_kvm.rs::layer1_probe_fixture_compiles_static_on_host`
- `crates/m80-firecracker/tests/layer1_guest_smoke_real_kvm.rs::layer1_guest_known_cve_primitives_are_blocked_in_guest`
