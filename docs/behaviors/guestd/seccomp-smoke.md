# Guestd Seccomp Smoke

Behavior capture for bead `m80-8emae.14.4`.

The real-KVM smoke boots a minimal image whose PID 1 is the current
`m80-guestd`, waits for readiness, and then proves the complete fixed-profile
cutover from inside the guest:

- long-lived PID 1 reports `Seccomp: 2` after readiness
- buffered exec, streaming exec, and PTY exec can still run ordinary
  `/bin/cat` workloads
- each workload path reports UID/GID `1000`, `NoNewPrivs: 1`, `Seccomp: 2`,
  and empty inheritable, permitted, effective, and bounding capability sets
- the fixed workload profile blocks the selected denied syscall probe:
  `/m80-guestd --m80-seccomp-probe workload deny-unshare`

Smoke command:

```sh
sudo -n env \
  M80_FORCE_PREFLIGHT=1 \
  M80_JAIL_UID=1000 \
  M80_JAIL_GID=1000 \
  M80_RUN_ROOT=/var/lib/m80-run \
  M80_KERNEL_IMAGE=/opt/m80/artifacts/vmlinux \
  M80_ROOTFS_IMAGE=/opt/m80/artifacts/output.ext4 \
  target/debug/deps/guestd-31c80f29ec12ae9b \
  --ignored real_kvm_guestd_seccomp_preserves_exec_paths \
  --nocapture --test-threads=1
```

Observed result on 2026-05-14:

```text
test seccomp_smoke::real_kvm_guestd_seccomp_preserves_exec_paths ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out
```

Artifact evidence:

- `target/x86_64-unknown-linux-musl/release/m80-guestd` sha256:
  `55234d7ed14f050216d6282565fcb93d63b1e2848b4400e99a8a015c309b8aa6`
- `/opt/m80/artifacts/m80-guestd` sha256:
  `55234d7ed14f050216d6282565fcb93d63b1e2848b4400e99a8a015c309b8aa6`
- `/opt/m80/artifacts/output.ext4` sha256:
  `fbf52863aaedc835dafc0778e6ce8727484aad6c040a14f603ac3b364cb96b6b`

Test:

- `crates/m80-firecracker/tests/guestd/seccomp_smoke.rs::real_kvm_guestd_seccomp_preserves_exec_paths`
