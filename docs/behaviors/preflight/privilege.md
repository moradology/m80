# Privilege Preflight

`m80-preflight` refuses startup unless the process already has the host
privilege needed for Firecracker, jailer, TAP/bridge setup, cgroup enrollment,
device-node setup, ownership repair, and cleanup. Privilege is acquired before
m80 starts; m80 does not include a per-call sudo or privileged-command shim.

## Deployment Patterns

### Run as root

Run the m80 process with effective uid 0:

```text
sudo m80 run -- ...
```

or run m80 from a root-owned service/container. This satisfies
`PrivilegeStatus::Root`.

### Capability-bearing binary

Install the m80 binary at a stable path and grant the required effective file
capabilities:

```text
sudo install -m 0755 target/release/m80 /usr/local/bin/m80
sudo setcap cap_net_admin,cap_sys_admin,cap_mknod,cap_chown,cap_fowner,cap_kill,cap_setuid,cap_setgid,cap_setpcap+ep /usr/local/bin/m80
getcap /usr/local/bin/m80
```

The process must observe every capability in `REQUIRED_CAPABILITIES` in its
effective set. Missing any one of them returns
`PreflightError::PrivilegeUnavailable`.

`CAP_SETPCAP` is consumed at hardening boundaries: it lets `m80-jailer-harden`
drop unneeded capabilities from the official jailer's bounding set, and it lets
`m80-firecracker` drop `CAP_NET_ADMIN` from the backend thread after
`m80-net-helper` starts. `CAP_NET_ADMIN` is retained by the helper boundary for
outbound launch and cleanup; the parent must not keep it after backend
initialization.

### Privileged container capability set

When m80 runs inside a container, grant the same capabilities through the
container runtime. A Kubernetes pod security context must add the required set:

```yaml
securityContext:
  capabilities:
    add:
      - NET_ADMIN
      - SYS_ADMIN
      - MKNOD
      - CHOWN
      - FOWNER
      - KILL
      - SETUID
      - SETGID
      - SETPCAP
```

The container also needs the host resources that m80 uses, including `/dev/kvm`
and the configured artifact and run-root mounts.

## KVM Access Is Separate

The privilege gate does not replace `/dev/kvm` access. `m80-preflight` separately
checks that `/dev/kvm` exists and is writable by the current process. On a
non-root host install, the operator usually also needs kvm group membership:

```text
sudo usermod -aG kvm "$USER"
```

That group change controls KVM device access; it does not grant the Linux
capabilities required for jailer and networking operations.

## Evidence

- `crates/m80-preflight/src/lib.rs::classify_privilege`
- `crates/m80-preflight/tests/preflight/kvm_and_os_gates.rs`
