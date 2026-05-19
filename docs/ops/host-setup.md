# Host Setup

This document is the production operator checklist for hosts that build,
install, or run m80. m80 can validate many launch preconditions at runtime, but
it cannot make a compromised build host, broad operator account, or hostile
dependency source trustworthy after the fact.

## Identity Model

Use three identities with separate credentials and host permissions:

- **Build identity**: can build release artifacts and may need Docker for
  `m80 kernel-build` or image construction. Treat Docker daemon access as host
  root: an account that can use `/var/run/docker.sock` can run a privileged
  container and mount `/`. In short: Docker daemon access as host root.
- **Deploy identity**: can install signed m80 binaries, Firecracker binaries,
  guest artifacts, systemd units, and capability bits. It does not need Docker
  after artifacts already exist.
- **Run identity**: starts m80 services or CLI workloads. It needs only the
  runtime privilege shape admitted by `m80-preflight`: root, the documented
  Linux capability set, or a container runtime granting that set. It must not
  hold Docker socket access.

Do not run production m80 with the same account that builds arbitrary images or
has persistent Docker daemon access. Docker membership is an escalation path
outside m80's control boundary.

## Artifact Ownership And Modes

Install m80-owned paths as root-owned or deploy-owned and not group/world
writable:

```sh
sudo install -d -m 0755 /opt/m80
sudo install -d -m 0755 /opt/m80/bin
sudo install -d -m 0755 /opt/m80/artifacts
sudo install -d -m 0755 /opt/firecracker/bin
sudo install -d -m 0750 /var/run/m80
```

Required runtime artifacts:

- `/opt/firecracker/bin/firecracker`
- `/opt/firecracker/bin/jailer`
- `/opt/firecracker/bin/firecracker-seccomp-filter.bin`
- `/opt/m80/bin/m80-jailer-harden`
- `/opt/m80/bin/m80-net-helper`
- `/opt/m80/bin/m80`
- `/opt/m80/artifacts/host-binaries.manifest.json`
- kernel/rootfs/manifest/build-receipt artifacts under `/opt/m80/artifacts`

`/opt/m80/artifacts` is the boot-artifact trust boundary. Do not allow the run
identity or untrusted build jobs to replace files there after preflight has
accepted them. The run root must not be world-writable; it holds VM sockets,
diagnostics, overlays, and stopped-VM residue.

Host binaries and Firecracker launch material are also part of the launch TCB.
Install binaries as `root:root`, mode `0755` or narrower; install the
Firecracker seccomp filter as `root:root`, non-empty, regular, and not
group/world writable. Then generate
`/opt/m80/artifacts/host-binaries.manifest.json` from the installed bytes. The
exact manifest schema and install commands are in
`docs/ops/binary-installation.md`.

Each built rootfs must travel with `<rootfs>.manifest.json` and
`<rootfs>.build-receipt.json`. The receipt pins the manifest sha256; it is only
strong against co-replacement when the build/deploy process prevents the run
identity from rewriting the receipt after promotion.

## Cargo And Rust Supply Chain

Builds must use a committed lockfile and an audited dependency source. For
production releases, prefer vendoring:

```sh
cargo vendor vendor/
cargo build --locked --offline --release
```

Review every effective Cargo source override before trusting a build host:

```sh
cargo config get source
test ! -f ~/.cargo/config.toml || sed -n '1,200p' ~/.cargo/config.toml
```

A compromised build account can add:

```toml
[source.crates-io]
replace-with = "hostile"
```

and redirect future builds to an attacker-controlled registry. CI and release
builders should either ignore user-home Cargo config or run in a clean
container with explicit vendored sources. Production release jobs should fail
when `cargo build --locked --offline` needs network access.

## Build Environment

Treat the build host as part of the TCB for every emitted binary, kernel, and
rootfs. A reproducible build story is an operator posture, not a runtime m80
feature.

Recommended release build shape:

- clean container or clean VM for each release build;
- pinned Rust toolchain and target packages;
- vendored Rust dependencies;
- no network during the compile/package phase;
- signed artifact bundle containing m80 binaries, Firecracker version, kernel,
  rootfs, manifest, and build receipt;
- independent rebuild or hash comparison for release promotion when the threat
  model requires it.

If the same developer laptop builds artifacts, runs tests, and launches
production workloads, that laptop is in the production TCB.

## Runtime Host Preconditions

`m80 preflight` checks the launch-critical subset. Operators should still set
and record these host assumptions before production use:

- Linux kernel 6.1 or newer.
- `/dev/kvm` exists and is writable by the run identity.
- unified cgroup v2 when `M80_CGROUP_MODE=unified-v2`.
- configured jail UID/GID exist and are not root.
- `/opt/m80`, `/opt/firecracker`, artifact directory, and run root have the
  ownership/mode policy above.
- `vhost_vsock`, TUN, bridge, `tap`, and `nf_conntrack` support are present.
- `net.netfilter.nf_conntrack_max` is sized for expected concurrent VMs and
  outbound connections.
- CPU side-channel status is acceptable for the deployment; `m80-preflight`
  hard-fails selected `Vulnerable` rows unless the operator explicitly sets the
  documented skip env.
- SMT, KSM, transparent hugepages, CPU governor, and KVM halt-polling settings
  are chosen deliberately. See `docs/ops/host-tuning.md`.

Example inspection commands:

```sh
uname -r
ls -l /dev/kvm
stat -c '%U %G %a %n' /opt/m80 /opt/m80/artifacts /opt/firecracker/bin /var/run/m80
cat /proc/sys/net/netfilter/nf_conntrack_max
m80 preflight
```

## Runtime Privilege

For non-root execution, grant exactly the current m80 preflight capability set
to the installed binary or container:

```sh
sudo setcap cap_net_admin,cap_sys_admin,cap_mknod,cap_chown,cap_fowner,cap_kill,cap_setuid,cap_setgid,cap_setpcap+ep /usr/local/bin/m80
getcap /usr/local/bin/m80
```

`CAP_NET_ADMIN` is needed only long enough for `m80-firecracker` to start the
pinned `m80-net-helper`; backend initialization then drops it from the
backend thread. Keep `CAP_SETPCAP` in the launch set so that drop can remove
`CAP_NET_ADMIN` from that thread's bounding, permitted, and effective sets.

Do not combine the run identity with broader host powers such as Docker daemon
access, passwordless sudo, or write access to the artifact directory.

## CI Hardening

CI that publishes m80 artifacts is a deploy authority. Minimum posture:

- pin third-party GitHub Actions by commit SHA, not floating tags;
- build with `--locked` and fail if the lockfile changes;
- use vendored dependencies or an explicitly trusted registry mirror;
- isolate release secrets from pull-request jobs;
- produce and retain artifact hashes and the exact Firecracker version used;
- run `cargo audit` and the m80 release smoke suite before publishing;
- require human review for workflow changes that affect artifact production or
  publishing credentials.

Pinning GitHub Actions throughout the repository is tracked separately; this
document records the production requirement.
