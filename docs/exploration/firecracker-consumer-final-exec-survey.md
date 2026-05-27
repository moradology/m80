# Firecracker Consumer Final-Exec Survey

Date: 2026-05-27

Scope: identify whether a Firecracker jailer final-exec hardening patch has
co-beneficiaries beyond m80. This is not a full consumer threat-model audit; it
only looks for public evidence that final VMM process isolation matters to other
Firecracker or adjacent microVM users.

Source pins:

- firecracker-containerd:
  `5baa940fdf243f368a88f766786d9dd31323fc04`
- Kata Containers: `238dd51039d1ab40628a2348f07e5d650763c13d`
- Ignite: `f60dc65ed25380ea696f817fc44b949dceafd9bb`
- libkrun: `e67e5bb7558e566324917bbe3dc4877c87a9b70d`

## Summary

| Consumer | Final-exec posture | Cite as co-beneficiary? |
|---|---|---|
| AWS Lambda | Canonical Firecracker user, but relevant launch code is closed. Public material supports broad Firecracker isolation value, not final-exec details. | Broad only. Do not claim Lambda wants this exact patch. |
| firecracker-containerd | Strongest public evidence that VMM process jailing matters beyond m80. Its current implementation is runc-based, not proof it would consume an official-jailer flag immediately. | Yes, carefully. |
| Kata Containers Firecracker backend | Supports Firecracker and has a threat model centered on VM/VMM host isolation, capabilities, seccomp, and VMM-backed virtio devices. No direct final-exec jailer contract found. | Medium. Cite as an adjacent Firecracker VMM isolation beneficiary, not direct demand. |
| Weave Ignite | Archived. It historically preferred running Firecracker in a container rather than on the host with the official jailer. | No, except as historical evidence that users wanted VMM process sandboxing. |
| libkrun | Not a Firecracker jailer consumer. Its security model says guest and VMM share a security context and the VMM must be isolated by host OS features. | Adjacent only. Do not cite as Firecracker consumer demand. |

## AWS Lambda

Lambda is the canonical Firecracker production user, but its launch and jailer
integration are not publicly inspectable at the level this epic needs. It is
safe to say a stronger official jailer final-exec contract fits Firecracker's
general serverless isolation posture. It is not safe to claim Lambda specifically
needs or would use the proposed hook.

Use Lambda as broad motivation only.

## firecracker-containerd

firecracker-containerd is the strongest public evidence that this problem is
not m80-specific.

The project positions Firecracker microVMs as providing a KVM isolation layer
for container workloads
([README.md#L6-L11](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/README.md#L6-L11)).
Its roadmap explicitly includes constraining or jailing the Firecracker VMM
process to improve host security posture
([README.md#L56-L67](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/README.md#L56-L67)).

It has a `JailerConfig` in runtime configuration
([config/config.go#L56-L70](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/config/config.go#L56-L70))
and a jailer abstraction for modifying the Firecracker VM launch path and
exposing files into the jailed filesystem
([runtime/jailer.go#L36-L49](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/runtime/jailer.go#L36-L49)).
The current implementation uses `runc` to set up a jailed environment for
Firecracker
([runtime/runc_jailer.go#L65-L85](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/runtime/runc_jailer.go#L65-L85))
and launches Firecracker through that process runner
([runtime/runc_jailer.go#L176-L199](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/runtime/runc_jailer.go#L176-L199),
[runtime/runc_jailer.go#L502-L511](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/runtime/runc_jailer.go#L502-L511)).

Its host-file-isolation doc calls out the Firecracker and Jailer binaries as
launch/runtime files that should be protected externally to Firecracker/Jailer
([docs/host-file-isolation.md#L40-L46](https://github.com/firecracker-microvm/firecracker-containerd/blob/5baa940fdf243f368a88f766786d9dd31323fc04/docs/host-file-isolation.md#L40-L46)).
That is not the same as final-exec state, but it confirms the project cares
about host-side VMM process containment.

Conclusion: cite firecracker-containerd with care. It is a concrete
co-beneficiary for the broader VMM-process-isolation goal, not direct proof that
the project currently launches the official jailer or would adopt a new
official-jailer flag immediately. The proposal should say the hook benefits any
orchestrator that launches the official jailer and is aligned with public
firecracker-containerd pressure to keep the VMM process jailed.

## Kata Containers Firecracker Backend

Kata supports Firecracker as one of its hypervisors
([docs/hypervisors.md#L19-L24](https://github.com/kata-containers/kata-containers/blob/238dd51039d1ab40628a2348f07e5d650763c13d/docs/hypervisors.md#L19-L24))
and describes Firecracker as a lightweight hypervisor created for AWS Lambda
([docs/hypervisors.md#L43-L46](https://github.com/kata-containers/kata-containers/blob/238dd51039d1ab40628a2348f07e5d650763c13d/docs/hypervisors.md#L43-L46)).

Kata's threat model explicitly names namespaces, cgroups, capabilities, SELinux,
and seccomp as the traditional container isolation surface
([docs/threat-model/threat-model.md#L25-L40](https://github.com/kata-containers/kata-containers/blob/238dd51039d1ab40628a2348f07e5d650763c13d/docs/threat-model/threat-model.md#L25-L40)).
It also calls out VMM and host-kernel risk, including VMM-backed virtio-blk,
virtio-scsi, networking, and vsock paths
([docs/threat-model/threat-model.md#L89-L145](https://github.com/kata-containers/kata-containers/blob/238dd51039d1ab40628a2348f07e5d650763c13d/docs/threat-model/threat-model.md#L89-L145),
[docs/threat-model/threat-model.md#L166-L190](https://github.com/kata-containers/kata-containers/blob/238dd51039d1ab40628a2348f07e5d650763c13d/docs/threat-model/threat-model.md#L166-L190)).

No direct public claim was found that Kata needs a Firecracker jailer final-exec
hook. The co-beneficiary argument is indirect: Kata has a documented VMM
isolation threat model and Firecracker support, so a stronger official jailer
contract aligns with its security posture.

Conclusion: cite as adjacent support, not primary demand.

## Weave Ignite

Ignite is deprecated and points users to Flintlock
([README.md#L1-L4](https://github.com/weaveworks/ignite/blob/f60dc65ed25380ea696f817fc44b949dceafd9bb/README.md#L1-L4)).
It historically positioned Firecracker as its isolation layer
([README.md#L17-L22](https://github.com/weaveworks/ignite/blob/f60dc65ed25380ea696f817fc44b949dceafd9bb/README.md#L17-L22)).

Its FAQ says the project tried host `systemd` early, then chose to package and
run Firecracker in a container. It states that Firecracker should not be run on
the host without sandboxing, and that Firecracker provides a jailer, but Ignite
used a container for that job
([docs/FAQ.md#L60-L69](https://github.com/weaveworks/ignite/blob/f60dc65ed25380ea696f817fc44b949dceafd9bb/docs/FAQ.md#L60-L69)).
Current launch code has `JailerCfg` commented out and builds a direct
`firecracker` command
([pkg/container/firecracker.go#L50-L72](https://github.com/weaveworks/ignite/blob/f60dc65ed25380ea696f817fc44b949dceafd9bb/pkg/container/firecracker.go#L50-L72),
[pkg/container/firecracker.go#L103-L111](https://github.com/weaveworks/ignite/blob/f60dc65ed25380ea696f817fc44b949dceafd9bb/pkg/container/firecracker.go#L103-L111)).

Conclusion: not a useful upstream co-beneficiary in 2026. It is only historical
evidence that Firecracker users cared about VMM process sandboxing enough to
wrap it externally.

## libkrun

libkrun is adjacent, not a Firecracker jailer consumer. It is a library that
embeds a VMM and exposes KVM-based process isolation to applications
([README.md#L7-L24](https://github.com/containers/libkrun/blob/e67e5bb7558e566324917bbe3dc4877c87a9b70d/README.md#L7-L24)).
Its security model states that the guest and VMM share the same security
context, that the VMM can proxy host resources to the guest, and that host OS
isolation such as namespaces should isolate the VMM
([README.md#L85-L97](https://github.com/containers/libkrun/blob/e67e5bb7558e566324917bbe3dc4877c87a9b70d/README.md#L85-L97)).
It also acknowledges Firecracker/rust-vmm/Cloud Hypervisor code heritage
([README.md#L270-L272](https://github.com/containers/libkrun/blob/e67e5bb7558e566324917bbe3dc4877c87a9b70d/README.md#L270-L272)).

Conclusion: use libkrun as an adjacent threat-model reference only. Do not cite
it as a Firecracker upstream consumer.

## Recommendation For The Upstream Proposal

The upstream proposal should lead with firecracker-containerd as the public
co-beneficiary, then mention Kata Containers as an adjacent Firecracker VMM
isolation beneficiary. Lambda can motivate the general Firecracker isolation
posture, but should not be used as a specific claim. Ignite and libkrun should
stay out of the main argument.

If upstream asks who uses this beyond m80, the accurate answer is: any
orchestrator that launches the official jailer and wants final VMM process state
to be owned by Firecracker rather than by an outer runc/systemd/wrapper layer.
firecracker-containerd is the clearest public evidence that this class of VMM
process isolation matters to Firecracker consumers, but its current runc-based
jailer path should be described honestly.
