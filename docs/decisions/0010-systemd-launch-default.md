# 0010 - Systemd Launch Default with Wrapper Fallback

## Context

m80 launches Firecracker through a four-step host-side chain:

```
m80 → m80-jailer (bind plan + chroot materialize)
    → spawn m80-jailer-harden (pre-exec hardening shim)
        → exec firecracker-jailer (official jailer: chroot, mknod, setuid)
            → exec firecracker (VMM inside the chroot)
```

`m80-jailer-harden` is a ~700-line privileged wrapper
(`crates/m80-jailer-harden/`) that applies hardening which inherits across
the upcoming `exec` into the official jailer. The wrapper exists because the
official jailer needs `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_SYS_CHROOT`,
`CAP_SETUID`, and friends to do its mount/mknod/chroot/uid-drop work;
pre-shrinking those caps would break the official jailer, but
pre-shrinking *everything else* and applying `NoNewPrivileges`, ambient-cap
clear, supplementary-group drop, signal mask reset, `close_range`, and
`env_clear` is fine and inherits cleanly.

Concretely, the wrapper applies:

- Prune the bounding capability set to the seven-cap official-jailer minimum
  (`CAP_CHOWN`, `CAP_DAC_OVERRIDE`, `CAP_SYS_CHROOT`, `CAP_MKNOD`,
  `CAP_SETUID`, `CAP_SETGID`, `CAP_SYS_ADMIN`)
- Clear inheritable and ambient capability sets
- Retain only the allowed seven in effective and permitted
- `PR_SET_NO_NEW_PRIVS`
- `PR_SET_PDEATHSIG=SIGKILL`
- `setgroups([])`
- `umask 0077`
- Signal mask reset
- `close_range(3, UINT_MAX, 0)`
- `env_clear()` before exec
- Optional `setrlimit` for `nproc`, `memlock`, `as`, `core`, `stack`
- Optional `unshare(CLONE_NEWCGROUP)` / `unshare(CLONE_NEWNET)`

Every wrapper directive is also expressible as a systemd unit directive, and
systemd ships additional hardening primitives the wrapper does not — at minimum
`RestrictNamespaces=`, `LockPersonality=`, `ProtectKernelModules=`,
`ProtectKernelTunables=`, `ProtectKernelLogs=`, `ProtectClock=`,
`RestrictAddressFamilies=`, `RestrictSUIDSGID=`, `KeyringMode=`,
`SystemCallArchitectures=`.

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
`systemd-run --unit=m80-vm-<id> --collect --property=...` per launch.
Per-VM-variable directives — rlimits derived from `JailerConfig`, optional
cgroup/net namespace requests — are inline `--property=` flags. The static
unit-template alternative was rejected because:

- A static `m80-vm@.service` would still need `--property=` overrides for
  the per-VM-variable directives, so it would double the audit surface
  (`.service` file plus Rust arg-builder) without simplifying anything.
- The transient form leaves the launch contract in one auditable place
  (a typed Rust function with directive-snapshot tests) rather than two.

`m80-net-helper` invocations use the same transient shape — one transient
unit per request, with `AmbientCapabilities=CAP_NET_ADMIN` plus the same
restricted directive envelope.

### B. systemd version floor

Floor: **245**. The full directive set commits to:

- `CapabilityBoundingSet=`, `AmbientCapabilities=`, `NoNewPrivileges=`,
  `UMask=`, `SupplementaryGroups=`, `Environment=`, `KeyringMode=`
- `LockPersonality=`, `RestrictNamespaces=`, `RestrictSUIDSGID=`,
  `RestrictAddressFamilies=`
- `ProtectKernelModules=`, `ProtectKernelTunables=`, `ProtectKernelLogs=`,
  `ProtectClock=`
- `SystemCallArchitectures=`, `SystemCallFilter=`
  (Firecracker-compatible profile; cannot be tighter than what Firecracker
  itself installs via `--seccomp-filter`)
- Per-launch `LimitNOFILE=`, `LimitFSIZE=`, `LimitNPROC=`, `LimitMEMLOCK=`,
  `LimitAS=`, `LimitCORE=`, `LimitSTACK=` from `JailerConfig`

The floor reflects the highest-required directive in the chosen set
(`ProtectClock=`). The implementing PR pins the exact version constant in
`crates/m80-preflight/src/checks.rs` and a test ties the constant to the
directive list — if the directive set ever grows to need newer systemd, the
floor moves with it and the test fails closed.

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
`--property=IOWeight=...`, `--property=TasksMax=...` cover static-at-launch
values only. Anything mid-flight — retroactive memory cap, dynamic CPU
adjustment, sub-cgroup creation per warm-pool slot, the cgroup-favordynmods
work — keeps direct cgroup writes. The DBus `set-property` round-trip is
too slow for the hot path; see `docs/perf/cgroup-microcuts.md`.

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
- **systemd present but below 245**: typed preflight error naming the
  observed version and the required version. Does *not* silently fall back
  to wrapper. Silent downgrade is a footgun — operators would believe they
  have systemd hardening and they would not.
- **Both paths present** (systemd ≥ 245 and the wrapper binary on disk):
  systemd path used; wrapper binary remains on disk unused. No harm.
- **systemd unit creation fails at launch** (DBus unreachable, transient-name
  collision, etc.): launch fails with a typed error. Does *not* fall back to
  the wrapper. Same reason: silent downgrade.

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

## Alternatives considered

| Alternative | Where it beats us | Where it loses | Verdict |
|---|---|---|---|
| systemd-driven launch (chosen) | Wide directive coverage; battle-tested code path; declarative; absorbs maintenance | Excludes hosts on systemd < 245 from the primary path | Default |
| `m80-jailer-harden` only (status quo) | Works anywhere with KVM; small audit surface; no version pin | Misses ~8 hardening primitives systemd ships; m80 owns the syscall code; cap-allowlist drift risk against upstream Firecracker jailer | Retained as fallback |
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
  `Command::new("systemd-run")` and `Command::new(jailer_harden_bin)`.
- New arg-builder module in `m80-firecracker` for the `systemd-run`
  invocation. Unit-tested for directive coverage against a pinned snapshot.
  The cap-bounding allowlist is constructed from the same
  `OFFICIAL_JAILER_CAPABILITIES` constant the wrapper uses, with a
  cross-crate test pinning the two against each other so they cannot
  drift.
- `m80-net-outbound` learns the same `chosen_launch_path` and branches
  its net-helper invocation accordingly.
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
unit names (`m80-vm-<id>.service`) in addition to its current pid-file
discovery, which is wrapper-path-only. This is implementation work in the
launch-branching child (`m80-9wm35.4`).

This shift is arguably an improvement for reliability — VMs no longer die
because the launcher hiccups — but it is a behavior change and warrants
explicit handling, not a silent one.

### What does NOT change

- `m80-guestd` PID-1 behavior. Guest image stays no-systemd per
  `docs/behaviors/image-build/minimal-image-design.md`.
- `m80-cgroup` hot path. Direct cgroup v2 writes stay.
- `m80-jailer` bind plan, chroot materialization, and recovery code. The
  host-side jailer crate is unaffected; both launch paths consume its
  `MaterializedJail` the same way.
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

The wrapper-only world owned ~700 lines of privileged Rust plus the
`support/m80-close-range/` helper. The systemd-first world owns the
arg-builder (smaller; pure data construction; no FFI) plus the
directive-snapshot test plus the cap-allowlist alignment test. The wrapper
survives behind the feature gate for the no-systemd population.

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
