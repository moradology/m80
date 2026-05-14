# Security Hardening Boundary Splits

Behavior beads: `m80-8emae.30`, `m80-8emae.34`, `m80-8emae.14`.

## Purpose

The remaining `m80-8emae` hardening items are process-boundary problems, not
single-flag launch changes. This document records the safe split so follow-up
work does not reintroduce the rejected shortcuts. The `AllowOutbound` topology
below has been implemented; the network-helper protocol, helper-backed
OutboundNat call routing, and parent `CAP_NET_ADMIN` drop are implemented.
The remaining split follow-up work is smoke evidence for the full outbound
path and the guestd seccomp boundary:

- moving an `AllowOutbound` TAP out of the host namespace without replacing the
  data path; implemented with an m80-owned namespace, private bridge, and veth
  pair;
- dropping `CAP_NET_ADMIN` from the long-lived m80 process after helper-backed
  launches and cleanup are live; implemented at backend initialization;
- installing seccomp in long-lived `m80-guestd` while workload `fork`/`exec`
  still inherits the daemon filter.

The goal remains generic VM mechanics. None of these boundaries may introduce
adapter tool catalogs, semantic IDs, or product policy.

## AllowOutbound Private VMM Netns

`NoEgress` uses `m80-jailer-harden --new-net-ns`, so the Firecracker VMM gets a
fresh empty network namespace. `AllowOutbound` cannot reuse that shape directly:
the owned outbound path needs a data path from a VMM-private TAP back to the
host bridge and NAT policy.

The implemented hard-cutover design is an m80-owned named namespace for each
outbound VM:

1. Create and persist a namespace handle before Firecracker launch.
2. In the host namespace, keep the run-root bridge, host NAT, and iptables
   ownership model.
3. Create a veth pair. Enslave the host end to the run-root bridge.
4. Inside the VMM namespace, create a small bridge and attach the private veth
   end plus the Firecracker TAP.
5. Pass the namespace path to the official jailer as `--netns`.
6. Tear down the per-VM namespace, private bridge, TAP, veth pair, iptables
   rules, and per-VM state from one recorded ownership state.

Evidence to keep current:

- unit tests pin link-operation order and rollback for the namespace topology;
- real-KVM `AllowOutbound` test proves Firecracker's `/proc/<pid>/ns/net`
  differs from the host namespace;
- real-KVM `AllowOutbound` test proves guest outbound traffic still works;
- cross-VM test continues to prove peer guest IPv4 is rejected.

## CAP_NET_ADMIN Lifetime

The PID namespace half of `m80-8emae.34` is already implemented. The remaining
host capability problem cannot be solved by clearing `CAP_NET_ADMIN` after one
launch phase: a long-lived backend may need later outbound launches, failed
launch rollback, stopped-VM delete, and startup orphan cleanup.

The safe hard-cutover design is a narrow network-ops helper boundary:

1. Discover an m80-owned helper executable during preflight. The helper is a
   pinned host binary, covered by the host-binaries manifest, with the same
   fail-closed path and sha256 checks as `m80-jailer-harden`; there is no
   fallback to shelling out through the parent.
2. Spawn the helper before the parent drops capabilities. The helper owns only
   the network operations requiring `CAP_NET_ADMIN`: bridge/veth/TAP/namespace
   setup, iptables/sysctl policy application, per-VM cleanup, and orphan
   bridge cleanup.
3. Move existing `m80-net-outbound` operations behind a finite request/response
   protocol with typed failure variants, request/response size caps, and
   unknown operation rejection. This protocol and the parent-side client are
   implemented.
4. After helper startup, the parent drops `CAP_NET_ADMIN` from effective,
   permitted, inheritable, ambient, and bounding sets. This is implemented in
   `m80-firecracker` backend initialization.
5. Parent launch and cleanup paths call the helper instead of running rtnetlink
   or iptables directly. Phase 6 realization, phase 7 policy application,
   launch rollback, stopped-sandbox delete/preserve cleanup, and stale run-root
   recovery are helper-backed.
6. Helper lifetime is bound to live backend handles through a process-global
   weak registry; helper exit poisons new outbound launches and triggers
   explicit cleanup diagnostics.

The initial helper operation set is intentionally small:

- realize the `OutboundNat` bridge/veth/private-netns/TAP topology;
- apply the host sysctl and iptables policy for a ready VM network state;
- clean one VM's owned network state;
- clean an orphan run-root bridge.

No caller-controlled tool catalog, adapter policy, arbitrary command execution,
or generic "network admin" RPC may enter this helper. If an operation is not in
the enum, it is unsupported.

Required evidence:

- parent process `/proc/self/status` no longer lists `CAP_NET_ADMIN` in
  `CapBnd`, `CapPrm`, or `CapEff` after backend initialization;
- OutboundNat launch, stop/delete cleanup, and orphan cleanup still pass through
  the helper;
- helper protocol rejects unknown operations and oversized requests;
- real-KVM outbound launch and delete pass after parent capability drop.

## Guestd Seccomp

Firecracker advanced seccomp is already implemented. The remaining guestd
seccomp work needs a broker/helper split. Installing a daemon seccomp filter in
the existing accept loop would be inherited by buffered exec, streaming exec,
and PTY workloads, which must remain arbitrary user-selected programs under
m80's fixed non-root workload profile.

The safe hard-cutover design is:

1. Start a workload broker before long-lived guestd installs its daemon filter.
2. Route buffered exec, streaming exec, and PTY spawn through that shared broker
   boundary; no path may keep an in-daemon direct spawn.
3. Install a tight seccomp filter in long-lived guestd after PID-1 setup,
   listener bind, ready signal setup, and broker startup.
4. Keep workload privilege policy fixed: UID/GID 1000, no new privs, empty
   capability sets. Workload seccomp is a separate fixed profile applied in the
   exec shim or broker child, not caller-controlled wire policy.
5. Bound broker request size and response size; broker crash poisons further
   exec requests with a typed guest-side failure.

The daemon filter and the workload filter are separate fixed m80 profiles. The
guest, host caller, and adapter consumer do not get a wire field that selects or
extends syscall policy. A future adapter-specific policy surface would need its
own promotion decision outside this security hardening cutover.

Required evidence:

- unit tests prove buffered exec, streaming exec, and PTY all use the broker
  spawn path;
- guestd self-test or real-KVM probe proves long-lived guestd has seccomp mode
  enabled after readiness;
- workload probe proves ordinary commands still run under the fixed non-root
  profile;
- denied-syscall probe proves the chosen workload or daemon profile blocks a
  syscall that was previously available.

## Tracker Split

The implementation should land as small leaves:

1. `DESIGN AllowOutbound private VMM netns topology`
2. `IMPL m80-net-outbound namespace topology and rollback`
3. `IMPL m80-firecracker AllowOutbound --netns launch plumbing`
4. `SMOKE AllowOutbound private netns with live outbound`
5. `DESIGN CAP_NET_ADMIN helper boundary`
6. `IMPL network-ops helper protocol`
7. `IMPL helper-backed launch/cleanup`
8. `IMPL parent CAP_NET_ADMIN drop after helper startup`
9. `SMOKE OutboundNat after parent capability drop`
10. `DESIGN guestd seccomp broker boundary`
11. `IMPL shared guestd workload broker for exec/streaming/PTY`
12. `IMPL guestd daemon seccomp and fixed workload seccomp profile`
13. `SMOKE guestd seccomp with arbitrary workload exec preserved`
