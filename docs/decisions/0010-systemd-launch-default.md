# 0010 - Systemd Launch Default with Wrapper Fallback

## Context

m80 launches Firecracker through a four-step host-side chain:

```
m80 → m80-jailer (bind plan + chroot materialize)
    → spawn m80-jailer-harden (pre-exec hardening shim)
        → exec firecracker-jailer (official jailer: chroot, mknod, setuid)
            → exec firecracker (VMM inside the chroot)
```

`m80-jailer-harden` is a privileged wrapper
(`crates/m80-jailer-harden/`; roughly 400 functional lines plus its tests)
that applies hardening which inherits across the upcoming `exec` into the
official jailer. The wrapper exists because the official jailer needs
`CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_SYS_CHROOT`, `CAP_SETUID`, and friends
to do its mount/mknod/chroot/uid-drop work; pre-shrinking those caps would
break the official jailer, but pre-shrinking *everything else* and applying
one-way process state before the official-jailer `exec` is useful.

Concretely, `apply_process_hardening` applies, in order:

- Optional `unshare(CLONE_NEWCGROUP)` / `unshare(CLONE_NEWNET)`
- Optional `setrlimit` for `no-file`, `fsize`, `nproc`, `memlock`, `as`,
  `core`, `stack`
- `setgroups([])`
- Clear inheritable and ambient capability sets
- Prune the bounding capability set to the seven-cap official-jailer minimum
  (`CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_SYS_CHROOT`, `CAP_MKNOD`,
  `CAP_SETUID`, `CAP_SETGID`, `CAP_SYS_ADMIN`)
- Retain only the allowed seven in effective and permitted
- `PR_SET_NO_NEW_PRIVS`
- `PR_SET_PDEATHSIG=SIGKILL`
- `umask 0077`
- Signal mask reset
- `close_range(3, UINT_MAX, 0)`

The wrapper's `exec_jailer` caller also clears the environment before
spawning the official jailer command.

Most wrapper behavior is expressible as systemd unit policy, and systemd also
ships hardening primitives the wrapper does not. The central case is narrower
than "systemd adds eight new protections": several directives close surfaces
the jailer geometry already reduces. The real incremental systemd coverage is
the per-unit keyring, personality, namespace, address-family, SUID/SGID, and
kernel-interface policy envelope around the privileged jailer window.

systemd is present and supported on roughly 95% of realistic m80 deployment
hosts (Ubuntu, Debian, RHEL family, Fedora, SUSE, Amazon Linux, all major
cloud distros). The remaining 5% are deliberate non-systemd choices: Alpine,
Void, Devuan, Artix, container-PID-1 deployments, embedded Yocto/Buildroot
rootfses. This ADR resolves how the launch hardening surface should be split
between the two regimes.

## Decision

The systemd-driven launch path is the default. The `m80-jailer-harden`
wrapper is retained as a feature-gated fallback for hosts that lack a
sufficiently modern systemd. The two paths are mutually exclusive at
preflight. Exactly one is chosen per host. There is no silent fallback.

### A. Static unit template vs. transient `systemd-run`

Fully transient. No installed unit files. The arg-builder in
`m80-firecracker` produces
`systemd-run --unit=m80-vm-<derived-token> --collect --property=...` per
launch.
Per-VM-variable directives — rlimits derived from `JailerConfig`, optional
cgroup/net namespace requests — are inline `--property=` flags. The static
unit-template alternative was rejected because:

- A static `m80-vm@.service` would still need `--property=` overrides for
  the per-VM-variable directives, so it would double the audit surface
  (`.service` file plus Rust arg-builder) without simplifying anything.
- The transient form leaves the launch contract in one auditable place
  (a typed Rust function with directive-snapshot tests) rather than two.

`m80-net-helper` uses the same transient mechanism but not the same lifetime:
one long-lived transient unit is started for the backend helper lifetime, and
requests continue to flow over the existing finite stdio protocol. The helper
unit carries `AmbientCapabilities=CAP_NET_ADMIN CAP_SYS_ADMIN`; `CAP_SYS_ADMIN`
is required for the named-network-namespace bind mounts under `/run/netns`.
The helper directive envelope deliberately avoids filesystem protection
directives that create a private mount namespace, because the official jailer
must be able to join the helper-created namespace by path.

### B. systemd version floor

Floor: **245**. The VM-launch directive set commits to:

- `CapabilityBoundingSet=`, `AmbientCapabilities=` (explicit empty for VM
  launch), `NoNewPrivileges=`, `UMask=`, `SupplementaryGroups=`,
  `KeyringMode=private`
- `LockPersonality=`, `RestrictSUIDSGID=`,
  `RestrictAddressFamilies=AF_UNIX AF_NETLINK AF_VSOCK`
- `ProtectKernelModules=`, `ProtectKernelTunables=`, `ProtectKernelLogs=`,
  `ProtectClock=`
- `SystemCallArchitectures=native`
- `StandardOutput=append:<console-log>`,
  `StandardError=append:<console-log>` when a VM console log is configured;
  otherwise both are `null`
- `Type=forking` plus `PIDFile=<jail-root>/<firecracker-basename>.pid` when
  the official jailer detaches for `new_pid_ns` or `daemonize`
- Per-launch `LimitNOFILE=`, `LimitFSIZE=`, `LimitNPROC=`, `LimitMEMLOCK=`,
  `LimitAS=`, `LimitCORE=`, `LimitSTACK=` from `JailerConfig`

The floor reflects the highest-required directive in the chosen set
(`ProtectClock=`). That means the floor depends on one directive with
relatively small incremental value because the wrapper and official jailer
already remove `CAP_SYS_TIME`; keeping it is a deliberate simplicity choice,
not the strongest security argument in the table. The implementing PR pins
the exact version constant in `crates/m80-preflight/src/checks.rs` and a test
ties the constant to the directive list — if the directive set ever grows to
need newer systemd, the floor moves with it and the test fails closed.

The VM-launch path deliberately does **not** add an outer
`SystemCallFilter=` in Phase 1. The official jailer needs syscalls
Firecracker itself does not need (`mount`, `pivot_root`, `mknod`, namespace
setup), while Firecracker installs its own restrictive seccomp filter once
it starts. A looser outer filter adds little; a tighter one breaks the
jailer. Phase 2 (`m80-92eor`) owns any final-exec-site seccomp change.
For the same reason, the VM-launch path deliberately does **not** add
`RestrictNamespaces=`: the official jailer must be able to create or join the
mount, pid, and network namespaces that form the jail.
The VM-launch path also does **not** add `PrivateDevices=yes`: the official
jailer creates the device nodes that Firecracker needs inside the chroot, and
systemd's private device sandbox blocks that setup.

Excluded from the systemd path:

- RHEL 8 / CentOS 8 / Rocky 8 / Alma 8 (systemd 239)
- Amazon Linux 2 (systemd 219)
- Older long-tail enterprise builds (any systemd < 245)

Covered by the systemd path:

- Ubuntu 20.04 LTS and newer
- Debian 11 (bullseye) and newer
- RHEL 9 / Rocky 9 / Alma 9 and newer
- Fedora 33 and newer
- openSUSE Leap 15.3 and newer
- Amazon Linux 2023

RHEL 8 family upstream EOL is 2029. The wrapper fallback covers it until then.
NixOS, Fedora CoreOS, Flatcar, Talos, and Bottlerocket are not rejected by
name; they pass only if the live host exposes a root system bus and can create
a transient unit with the chosen directive set. Immutable/declarative service
management is an operator packaging concern, not a separate m80 launch path.

### C. m80 backend launch model

The m80 backend itself is launcher-agnostic. The systemd-first scope is how
m80 launches its *child* processes (jailer chain, net-helper), not how the
backend is launched. Operators may invoke m80 from a CLI, an init.d script, a
custom launcher, or a systemd unit; the only systemd dependency is
`systemd-run` being available at runtime when the systemd path is chosen.

A recommended `packaging/systemd/m80.service` example ships as documentation
for operators who want unit-managed backend hardening. It is not a required
install and preflight does not check for it.

### D. m80-cgroup scope

`m80-cgroup` retains direct cgroup v2 writes for hot-path knobs.
`systemd-run --property=MemoryMax=...`, `--property=CPUWeight=...`,
`--property=IOWeight=...`, `--property=TasksMax=...` can cover
static-at-launch values only. Anything mid-flight — retroactive memory cap,
dynamic CPU
adjustment, sub-cgroup creation per warm-pool slot, the cgroup-favordynmods
work — keeps direct cgroup writes. The DBus `set-property` round-trip is
not the core argument: `docs/perf/cgroup-microcuts.md` measured only a small
P50 delta and no material tail change. The retained boundary is ownership:
m80 already owns dynamic per-VM cgroup topology and direct controller writes,
while systemd transient-unit properties cover launch-time service envelope.

### E. Rootless / user-mode systemd

Not supported. Root system bus is required. m80 already requires root for
the jailer chain (`mknod` for `/dev/kvm` and `/dev/net/tun`; capability
operations; mount setup). Adding a third weaker-hardened path for
rootless systemd would not satisfy the same threat model and is not worth
the complexity. Preflight rejects rootless-only systemd as if systemd were
absent; operator falls through to the wrapper path or fails preflight.

### F. Failure modes

All degraded states fail closed.

- **Both paths absent** (no systemd ≥ 245, no wrapper binary installed):
  typed preflight error pointing at install docs.
- **systemd present but below 245, wrapper present**: preflight explicitly
  chooses the wrapper path and reports systemd as unavailable for the primary
  path. This is not silent fallback; the chosen path is recorded in
  `Discovery` and the preflight table.
- **systemd present but below 245, wrapper absent**: typed preflight error
  naming the observed version, required version, and missing fallback.
- **Both paths present** (systemd ≥ 245 and the wrapper binary on disk):
  systemd path used; wrapper binary remains on disk unused. No harm.
- **systemd unit creation fails at launch** (DBus unreachable, transient-name
  collision, DBus disconnect, malformed property, etc.): launch fails with a
  typed error. It does *not* fall back to the wrapper after preflight has
  selected systemd.

### G. Reversibility triggers

Conditions that warrant revisiting this decision:

1. A systemd CVE or upstream change removes or weakens a directive m80
   depends on.
2. Operator demand for a long-tail non-systemd distro (Alpine on the host
   as a primary deployment, not an edge case) crosses an actionable
   threshold.
3. A security regression is discovered in the chosen directive set itself.
4. m80's threat model expands such that Phase 1 hardening is insufficient
   even with systemd directives applied. At that point, Phase 2 work
   (see `m80-92eor`) is the gating concern, not directive set choice.

Reversal is mechanically a one-line change: `default = ["no-systemd-launch"]`
in `crates/m80-jailer-harden/Cargo.toml`, plus flipping the preflight
selection default. The triggers above frame *when* to consider reversal,
not the mechanics.

### H. Doctrine scope

This is new host-launch doctrine, not a reversal. The workspace did not
previously commit to a no-systemd-dependency posture for the host launch
path. The guest-image doctrine — `m80-guestd` runs as PID 1 without
systemd, per `docs/behaviors/image-build/minimal-image-design.md` — stays
unchanged and is out of scope for this ADR.

### I. Sequencing note for Phase 2

The Phase 2 investigation (`m80-92eor`) may prove the final-exec-site gap is
empty or smaller than expected because Firecracker already drops privilege and
installs its own seccomp filter at startup. That result would narrow the value
of Phase 1 but would not make Phase 1 useless: systemd still hardens the
privileged official-jailer setup window and the network helper. The
implementation may proceed, but the final release note must keep the claim
bounded to Phase 1 and must not imply that systemd closes final-exec-site
capability/seccomp ownership.

## Alternatives considered

| Alternative | Where it beats us | Where it loses | Verdict |
|---|---|---|---|
| systemd-driven launch (chosen) | Wide directive coverage; battle-tested code path; declarative; absorbs maintenance | Excludes hosts on systemd < 245 from the primary path | Default |
| `m80-jailer-harden` only (status quo) | Works anywhere with KVM; small audit surface; no version pin | Misses the systemd-only keyring/personality/namespace/address-family/kernel-interface envelope; m80 owns the syscall code; cap-allowlist drift risk against upstream Firecracker jailer | Retained as fallback |
| OCI container runtime (Kata-style, firecracker-containerd) | Strong declarative hardening via OCI runtime spec | m80 is itself the sandbox; layering an outer container doubles the work and pushes the security boundary out to whatever runs the container | Wrong shape for m80 |
| Inline `pre_exec` closure in `m80-jailer` | No extra binary on disk; same security delivered as wrapper | Loses the testable process boundary; raw FFI inside `m80-jailer` grows the audit-sweep-ineligible surface; needs a new safe-wrapper crate anyway, which is what the existing wrapper already is | Same code with worse testability |
| Generic sandbox wrapper (`minijail`, `nsjail`, `bubblewrap`) | Battle-tested upstream | ~10k LOC of C as a runtime dep, with per-distro variation; not part of Firecracker's documented launch path | Wrong dep surface |
| Patch upstream Firecracker's jailer (Phase 2 Path A) | Closes the final-exec-site gap this ADR cannot close | Multi-month upstream cycle with no certainty; gates only on Phase 2 not Phase 1 | Different scope; tracked separately in `m80-92eor` |
| Build an m80-owned launcher replacing the official jailer (Phase 2 Path B) | Full control of the final exec site | ~1000–2000 LOC of new safe-syscall-wrapper code; inherits every jailer CVE class; no seccomp-construction infrastructure exists in m80 today | Different scope; deferred |
| Do nothing | Zero maintenance | Drops NNP, fd closure, ambient-cap clear, supplementary-group drop, extended rlimits. Unacceptable for the multi-tenant hostile-guest threat model | Wrong for threat model |

## Consequences

### What changes

- New `HostPrerequisiteCheckId::Systemd` variant in
  `crates/m80-preflight/src/host_prerequisite_result/check_id.rs` and its
  `ALL`/`as_str`/`check_name` rows; pinned by the existing label-order
  test.
- New `check_systemd` function in `crates/m80-preflight/src/checks.rs`
  parallel to existing checks. Detects `systemctl --version` ≥ 245 and
  validates the DBus system bus is reachable.
- `Discovery` (and downstream consumers) carry a `chosen_launch_path`
  discriminator: `LaunchPath::Systemd | LaunchPath::Wrapper`.
- `m80-firecracker` launch site branches on `chosen_launch_path` between
  the preflight-resolved absolute `systemd-run` path and the wrapper path.
- New arg-builder module in `m80-firecracker` for the `systemd-run`
  invocation. Unit-tested for directive coverage against a pinned snapshot.
  The cap-bounding allowlist mirrors the wrapper's
  `OFFICIAL_JAILER_CAPABILITIES` list, with tests pinning the two so they
  cannot drift.
- `m80-net-helper` launch uses the same preflight-resolved
  `chosen_launch_path`, but it does not reuse the VM-launch directive set:
  the helper needs `CAP_NET_ADMIN`, `CAP_SYS_ADMIN` for named netns bind
  mounts, netlink, and a host-visible `/run/netns` handoff, not the official
  jailer's mount/chroot/mknod capability envelope.
- `crates/m80-jailer-harden/Cargo.toml` grows `[features]` with
  `default = []` and `no-systemd-launch = []`. Default workspace builds
  with the feature unset produce no wrapper binary.
- `host-binaries.manifest.json` schema grows a conditional indicator. The
  wrapper row is allowed-absent when the systemd path is the chosen
  path. The net-helper, Firecracker, and jailer rows remain required
  regardless.
- `M80_JAILER_HARDEN_BIN` env var becomes optional on systemd hosts.
  `M80_NET_HELPER_BIN` stays mandatory; the net-helper binary itself is
  unchanged, only the launch shape around it changes.
- `packaging/systemd/m80.service` ships as documentation for operators
  who want unit-managed backend hardening. Optional, not required.
- Real-KVM smoke (`scripts/smoke.sh`) runs on both paths. Kernel-touching
  diffs in the launch site require smoke evidence per `CLAUDE.md` and
  decision `0003`.

### Orphan and recovery semantics

A real semantic shift, called out explicitly so it isn't a surprise during
implementation.

The wrapper today applies `PR_SET_PDEATHSIG=SIGKILL` so that if the m80
backend dies before the wrapper has exec'd into the jailer (or in flight),
the wrapper process dies too. With systemd-managed transient units, the
spawned unit is adopted by systemd. If the m80 backend crashes after the
unit is created, the unit continues — the jailer finishes its setup, the
VM boots and runs to completion, and systemd auto-collects the unit when it
exits (the `--collect` flag).

Net effect: VMs survive an m80-backend crash on the systemd path, which
they do not on the wrapper path. m80's existing recovery code
(`crates/m80-jailer/src/recover.rs`) reconciles partial state on next
startup. The recovery story has to learn to discover live VMs via systemd
unit names in addition to its current pid-file discovery, which is
wrapper-path-only. The unit name cannot be derived solely from the run-dir
basename because two different parents can carry the same basename and
because systemd unit names have a restricted character set. The
implementation uses a collision-resistant, systemd-safe name derived from
the run directory and records it in the persisted plan/state before unit
creation.

This shift improves one reliability property — VMs no longer die merely
because the launcher hiccups — while removing the wrapper's `PDEATHSIG`
crash-safety property. That is a real behavior change and must be tested by
crash/recovery proof before the systemd path becomes the default.

### What does NOT change

- `m80-guestd` PID-1 behavior. Guest image stays no-systemd per
  `docs/behaviors/image-build/minimal-image-design.md`.
- `m80-cgroup` hot path. Direct cgroup v2 writes stay.
- `m80-jailer` bind plan and chroot materialization. Recovery does change:
  it must reconcile both pid-file-backed wrapper launches and
  systemd-unit-backed launches.
- The wrapper binary's internal logic. It survives unchanged behind the
  feature flag for the non-systemd path.
- The official Firecracker jailer's role. Both paths exec the official
  jailer; m80 does not replace it.
- Phase 2 final-exec-site capability/seccomp hardening of the Firecracker
  process itself. See `docs/design/jailer-group-b-exec-site.md` and the
  investigation epic `m80-92eor`. systemd inherits hardening across exec
  but cannot apply a stricter policy *between* the jailer exec and the
  Firecracker exec inside a single unit, so the Phase 2 gap is unchanged
  by this ADR.
- `CLAUDE.md` or `AGENTS.md` prohibition on systemd. There was no such
  prohibition; this ADR is new doctrine, not a reversal of an existing
  one.

### Maintenance trade

The wrapper-only world owned roughly 400 functional lines of privileged Rust,
the wrapper tests, plus the `support/m80-close-range/` helper. The
systemd-first world owns the arg-builder (smaller; pure data construction;
no FFI), the transient-unit recovery logic, the directive-snapshot test, and
the cap-allowlist alignment test. The wrapper survives behind the feature
gate for the no-systemd population.

The new cost: a systemd version pin (245). CI test paths exercising the
systemd launch must run on a host with systemd ≥ 245. The
`host-binaries.manifest.json` conditional indicator adds a small schema
surface.

The drop: maintenance of the wrapper's syscall sequence under "what if
upstream Firecracker adds an eighth required cap" pressure, on the
default path. The fallback still has it; the default no longer does.

## Cross-references

- `docs/design/jailer-group-b-exec-site.md` — Phase 1 / Phase 2 framing.
  This ADR is Phase 1 scope only.
- `docs/behaviors/jailer/inherited-hardening-coverage.md` — what the
  wrapper currently provides.
- `docs/behaviors/image-build/minimal-image-design.md` — guest-image
  no-systemd doctrine, unchanged by this ADR.
- `docs/decisions/0003-audit-sweep-eligibility.md` — kernel-touching diff
  rule applies to launch-site changes.
- `crates/m80-jailer-harden/src/lib.rs::OFFICIAL_JAILER_CAPABILITIES` —
  the cap allowlist the systemd arg-builder mirrors.
- `crates/m80-preflight/src/checks.rs` — where the new `check_systemd`
  lands.
- Implementation epic `m80-9wm35` — this ADR is its `.1` child.
- Phase 2 investigation epic `m80-92eor` — out of scope for this ADR.
