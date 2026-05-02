# 10 — Risks and open questions

## Risks (in approximate order of likelihood × impact)

### 1. The `network.rs` port takes longer than estimated

**Estimate**: 10-15 days for full `OutboundNat`.
**Risk**: 20+ days if the privilege model needs rethinking.

The 3,669-line file shells out heavily, makes unguarded assumptions
about the host (kernel modules loaded, sysctl writable, iptables vs
nftables), and bakes predecessor's directory layout into collision detection.
Most port problems will show up in cleanup paths under crash conditions.

**Mitigation**: defer `OutboundNat` to v0.2. v0.1 ships `NoEgress` only.

### 2. KVM availability friction on developer machines

**Where**: macOS (requires Lima with nested virt → Apple Silicon +
recent macOS), Windows (no path), older Intel Macs (no nested virt).

**Impact**: m80 quickstart fails for a meaningful fraction of
prospective users.

**Mitigation**:
- Honest README that this is Linux/KVM-first
- Lima recipe ships in v0.1 (already exists in predecessor; port verbatim)
- Document expected Lima setup time (~15 minutes first time)
- Don't pretend; "doesn't work on macOS Intel" is fine if loud

### 3. The image build pipeline is harder to make portable than it looks

`prepare-guestd-image.sh` chroots into a mounted ext4 and runs
`apt-get install python3 nodejs`. This requires:
- Loop device support (some CI runners don't have it)
- `mount` privileges
- Functioning apt + network in the chroot
- The host distro family matching the guest (currently both Ubuntu)

**Risk**: m80 v0.1 image build only works on Ubuntu hosts. Cross-distro
support is harder.

**Mitigation**:
- Document host requirements
- Provide a Docker-based fallback (`docker run --privileged ubuntu:22.04
  /m80-prepare-image.sh`) for hosts that can't loop-mount
- v0.2: explore using `mkosi` or similar instead of chroot

### 4. Two direct consumers in predecessor need maintenance during cutover

`worker-rs/src/executor_factory.rs` and
`sandbox-executor-rs/src/lib.rs`. Both are in active production paths.

**Risk**: cutover breaks worker startup or sandbox-executor startup.

**Mitigation**:
- Keep `m80-adapter` behind a feature flag for the first PR
- Land in stages: adapter + tests first, switch behind flag, run
  parallel for a release, then remove the old crate
- Cutover is 2-4 days of work but should not be one PR

### 5. The "writeback" model decision

m80 has to decide what `m80 run --workspace DIR` does on exit. Options:

**A. Like predecessor**: write back into the host directory atomically with
admissibility scan.

**B. Snapshot**: produce a tarball or ext4 image of the post-exec
state; user decides what to do.

**C. Bind-mount via virtiofs**: live shared filesystem; no writeback
phase.

(A) is what predecessor already does and what the code supports. It's
opinionated. (C) is the future and may eventually obsolete (A), but
virtiofs in Firecracker is still maturing.

**Recommendation**: ship (A) as default with `--writeback off` to skip.
Document (C) as a v1+ direction.

### 6. Test infrastructure on CI

Real Firecracker tests need `/dev/kvm`. GitHub-hosted runners do not
provide nested virt. Options:

**A. Self-hosted Linux runner with KVM**: most flexible, costs ops time.

**B. AWS/GCP nested-virt instance for CI**: works, costs $$, requires
secrets.

**C. Mocked tests only in CI; integration tests run nightly on a
dedicated host**: cheap, slower feedback.

**Recommendation**: (C) for v0.1, upgrade to (A) when contributors join.

### 7. `agent-guest-proto` schema version 1 lock-in

Predecessor's `GuestRequest` is at schema version 1. If predecessor evolves it to
v2, the m80 adapter must keep speaking v1 to the m80 guest, or m80 must
upgrade.

**Risk**: m80 falls behind predecessor's protocol.

**Mitigation**: m80 ships its own protocol (`m80-proto`). Predecessor's
adapter handles `agent-guest-proto` separately if it ships its own
guest daemon image. The two never directly conflate.

### 8. Privileged operations expand the attack surface

m80 expects effective root or passwordless sudo. Users will run it on
shared boxes. iptables modifications are persistent across reboots
unless cleanup runs.

**Mitigation**:
- `m80 preflight` warns if privileges are excessive
- Cleanup is idempotent and tagged with comment prefix
- Document explicit cleanup: `m80 cleanup --everything`

### 9. Vendor lock to Firecracker

m80 is named after Firecracker. If users want Cloud Hypervisor or
QEMU-microvm support later, the abstraction has to extend.

**Mitigation**: not a v0.1 concern. m80 ships a `Sandbox` trait so a
future `m80-cloud-hypervisor` could share most plumbing. Don't generalize
prematurely.

### 10. The `agent-domain::CapabilityClass` resolution

`network.rs:159-161` reads `CapabilityClass` to decide network mode.
Replacing with `bool` is one-line, but the resolver also handles
`OutboundNatConfig` (exception CIDRs) which has more shape.

**Mitigation**: m80 v0.1 takes `--network none` only. v0.2 takes
`--network outbound-nat --allow-cidr 10.0.0.0/8`.

## Open questions for the user

These need a decision before extraction starts:

### Q1: Repo layout — separate repo or git submodule?

**Recommendation**: separate repo. m80 should be installable without
cloning predecessor. If extraction reveals lots of churn-coupling, revisit.

### Q2: Naming

"m80" is a working name. Other options: `firesbx`, `microsbx`, `fcsbx`,
`fcrun`, `microvm`, `vsbx`. Worth deciding before any public commits.

### Q3: License

Default suggestion: dual MIT / Apache-2.0 (Rust ecosystem norm). Confirm
or pick something else.

### Q4: Phase 1 scope — drop or include `jailer.rs`?

Including jailer adds ~1 week but is the right production posture.
Excluding lets v0.1 ship sooner but is "dev only".

**Recommendation**: include. The crate is portable as-is and
production-readiness matters even for a v0.1.

### Q5: Phase 2 ordering — OutboundNat first or snapshots first?

Both are roughly 2 weeks. Snapshots give the CLI a cool feature; NAT
gives users a complete deployment story.

**Recommendation**: NAT first. Most "I want to use a sandbox" use cases
need network.

### Q6: Does predecessor's adapter ship its own guest daemon?

**Option A**: predecessor keeps `guestd-rs`. m80 ships `m80-guestd`. Two
binaries, two protocols. Clean but a maintenance tax.

**Option B**: m80 only ships the host side. predecessor's existing
`guestd-rs` is the in-VM half, talking `agent-guest-proto`. The m80
host-side library is generic enough to drive any vsock-NDJSON daemon.

**Recommendation**: A. m80 must work standalone; relying on predecessor's
guest daemon makes m80's CLI unusable without predecessor.

### Q7: Distribution

Where does the binary go? Options:
- crates.io for `cargo install m80-cli`
- GitHub Releases with prebuilt binaries
- Both

**Recommendation**: both. Ship `cargo install` first; prebuilts in v0.2.

### Q8: Dependency on Firecracker version

Firecracker is on a regular release cadence and has occasional API
changes. Pinning v1.15.1 today; what's the upgrade story?

**Recommendation**: v0.1 pins one version. Tests run against the pinned
version. Bumping is a deliberate PR. Not auto-detect.

### Q9: What about Cloud Hypervisor?

CH has a similar API and is increasingly popular. m80 *could* support
both, but that doubles the implementation surface.

**Recommendation**: defer indefinitely. Different tool, different repo
if needed.

## Things to validate before committing

- [ ] Does `cargo build -p agent-sandbox-firecracker` work in the
  current predecessor checkout? (Confirm the codebase is in a good state to
  extract from.)
- [ ] Is there a recent run of the full integration test suite that
  passed? (Establishes a baseline.)
- [ ] Is the firecracker upstream at v1.15.1 still the recommended
  version? (Check upstream release status.)
- [ ] Does any contributor or stakeholder care about non-Linux hosts?
  (Confirms scope.)
- [ ] Is there an existing security review of the predecessor firecracker
  crate that should inform m80's posture? (Inherits assumptions.)
