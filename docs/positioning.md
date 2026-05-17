# m80 Positioning

## Summary

m80 is a generic Firecracker sandboxing foundation. It should feel simple at
the edge:

```sh
m80 run --workspace . --egress outbound -- echo hello
```

Under that command, m80 owns VM mechanics: validating boot artifacts, preparing
rootfs and scratch storage, launching Firecracker through the jailer, connecting
to guestd over vsock, mirroring stdout/stderr, returning the guest exit code,
capturing diagnostics, and tearing down owned host residue.

m80 does not own the product semantics layered above those mechanics. Tool
catalogs, idempotency keys, workspace authority, semantic agent events, and
policy decisions belong in adapters or callers.

## The Layer Boundary

The core rule is:

- VM and host mechanics belong in m80.
- Caller semantics belong above m80.

Examples that belong in m80:

- run a process in a selected guest image
- expose one workspace directory at `/workspace`
- choose `none` or `outbound` egress
- stream stdout and stderr or allocate a PTY
- read, write, stat, list, remove, and chunk-upload guest files
- preserve run-root evidence on failure
- recover stale owned run-root state
- lease a clean warm VM from an explicit warm owner

Examples that do not belong in m80:

- mapping `bash`, `exec`, `read_file`, or `write_file` tool names to policy
- deciding whether a tool call is read-only or mutating
- carrying `tool_call_id`, `workspace_id`, idempotency keys, or authority leases
- emitting `sandbox_exec_*` semantic events
- deciding when a caller may commit extracted workspace changes
- maintaining a product-specific HTTP API, registry, or SDK object model

This makes the core easier to audit. It also lets different products build
different surfaces without asking m80 to know their policy language.

## Adapter Pattern

Adapters are thin product-facing layers over the m80 core. They translate a
domain vocabulary into m80's generic execution vocabulary.

### Predecessor Adapter

A future `m80-adapter` can own the agent-specific contract:

- validate the tool catalog
- attach semantic identifiers and idempotency rules
- decide workspace writeback authority
- emit agent lifecycle events
- choose between cold run, warm stateless run, and persistent session run

The adapter should call m80 for the mechanics: launch, exec, PTY, file ops,
workspace projection, egress policy, diagnostics, and cleanup.

The concrete handoff is specified in [adapter-boundary.md](adapter-boundary.md).

### CI Runner

A CI adapter can present "run this untrusted pull request" while using m80 for:

- read-only source checkout projection
- bounded writable scratch
- outbound egress selection
- stdout/stderr/exit passthrough
- preserved run-root diagnostics on failure

The CI system owns job queues, secrets policy, artifact upload, and status
reporting.

### Multi-Tenant SaaS Plugin Host

A SaaS adapter can use m80 to run untrusted plugins with a deliberately narrow
view of the host:

- explicit input files
- explicit environment
- explicit egress mode
- direct output files or guest file reads
- cleanup evidence for leak detection

Tenant identity, billing, authorization, and audit events remain above m80.

### Language Bindings

Language bindings should wrap the generic surface, not invent agent semantics
inside the repository. A binding can expose a comfortable process/file API over
m80's Rust crates while leaving product-specific policy to the application.

The supported seams are described in [bindings.md](bindings.md).

## Comparison: SmolVM

The local SmolVM exploration found a useful contrast:

- SmolVM is SDK-shaped and product-shaped. It has a broad CLI, a Node embedded
  SDK, HTTP routes, file transfer APIs, registry-shaped commands, and an
  agent-branded user experience.
- Its wire verbs are still mostly VM mechanics: exec, file movement, lifecycle,
  logs, images, and status. The exploration found no tool catalog,
  `EffectClass`, idempotency-key contract, or workspace-authority state machine
  in the VM layer.
- SmolVM's public surface is easier for some first-touch AI sandbox use cases.
  m80's public surface is narrower and lower level by design.

Use SmolVM-shaped products when you want an opinionated SDK-first AI sandbox
surface and are comfortable with that product model.

Use m80 when you want a Rust-embeddable, Firecracker-specific foundation with a
small audit surface, typed lifecycle APIs, explicit host visibility controls,
and room to build your own adapter semantics above it.

## Comparison: Kata Containers

The local Kata exploration reinforces the same boundary from the opposite
direction:

- Kata is an OCI/containerd runtime with a large shim and guest-agent contract.
- It has substantial policy machinery for container and confidential-computing
  workflows, but not an agent tool-catalog contract in the VM mechanics layer.
- Kata abstracts across hypervisors because containerd deployments need that
  flexibility. m80 stays Firecracker-only through v0.x to avoid that abstraction
  tax and keep snapshot/warm-pool work direct.

Kata is the right reference when the product is OCI/containerd integration.
m80 is the right shape when the product wants to embed Firecracker sandboxing
directly without becoming a container runtime.

## Non-AI Use Cases

m80 should remain useful even if no agent product exists:

- untrusted pull-request test execution
- build farm isolation
- plugin and extension sandboxes
- tenant-specific data transforms
- reproducible security regression environments
- controlled outbound-network experiments

These use cases need the same mechanics as agents: small VM launch, explicit
filesystem visibility, explicit egress, stdout/stderr/exit fidelity, file
movement, diagnostics, and cleanup. They do not need agent semantic fields in
the core.

## Trust-Domain Assumption for Shared Pmem

`PmemSharing::Shared` is a same-trust-domain optimization, not a tenant
isolation primitive. m80 assumes guests sharing a `PmemSharing::Shared` image
share a trust boundary, because shared DAX-backed pages create a cache-timing
side channel between those guests. Cross-tenant isolation between
Shared-pmem-sharing guests is out of scope for m80; callers that need
cross-tenant isolation must use per-VM backing instead.

The acknowledgement for this tradeoff is part of construction, not prose in a
config comment. The shared mode is gated by a typed witness with finite
`TrustReason` variants; free strings and implicit defaults are deliberately not
accepted.

## Product Surface Implications

The m80 repository should continue to prioritize:

- `m80 run` as a process-transparent CLI facade
- `m80-firecracker` as the Rust orchestration API
- direct file operations and real streaming/PTY support
- explicit warm-owner lifecycle rather than hidden daemon behavior
- diagnostics that expose guest and host evidence before speculation
- crate READMEs as black-box contracts

Operator-facing layered-rootfs guidance lives in
`docs/runbook/layered-rootfs-operations.md` and
`docs/runbook/migrating-to-pmem-and-snapshot-restore.md`. Those runbooks keep
the same boundary as this document: they explain when to use PerVm, Shared, and
snapshot-template restore, but they do not introduce agent policy or
cross-tenant Shared-pmem semantics.

The m80 repository should not add:

- `m80 agent run --tool ...`
- in-core tool catalogs
- agent semantic event names
- idempotency or authority state machines
- product-specific HTTP APIs
- compatibility shims for unknown future consumers

Those surfaces can still exist. They just belong in adapters that depend on
m80, not in m80 itself.
