# External Language Binding Seams

## Purpose

Language bindings are useful, especially for Python-heavy agent and automation
workloads. They should live outside this repository unless they are a thin test
or example over the generic m80 surface.

The m80 repository owns stable Rust and CLI mechanics. External binding
packages own language packaging, SDK object models, retry policies, and product
semantics.

## Supported Binding Paths

### Rust Library Embedding

The strongest binding seam is the Rust API:

- `m80-firecracker::Backend` admits sandboxes.
- `Sandbox::launch` creates a `RunningSandbox`.
- `RunningSandbox::exec` and streaming/PTY variants run processes.
- direct file-operation methods read, write, list, stat, remove, and upload
  guest paths.
- warm-pool APIs lease clean pre-restored slots for low-latency stateless work.

A native binding can wrap these APIs directly when it is prepared to ship Rust
code and native dependencies.

### CLI Subprocess Embedding

The lower-integration seam is the CLI:

```sh
m80 run --workspace . --egress outbound -- python -c 'print("hello")'
```

This path is appropriate for scripts, test harnesses, and early SDK prototypes.
It preserves the process-wrapper contract: stdout is stdout, stderr is stderr,
and the exit code is the guest process exit code.

The CLI seam is intentionally generic. It does not expose `--tool`, product
event names, idempotency keys, or workspace-authority state.

### Adapter-Owned SDK

An adapter package can expose a product-shaped SDK, such as:

- `Sandbox.run_tool(...)`
- `Sandbox.read_file(...)`
- `Sandbox.write_file(...)`
- `with_sandbox(...)`
- `quick_exec(...)`

Those names are allowed in the adapter package. They should translate to m80's
generic operations at the boundary described in `adapter-boundary.md`.

## Python Binding Guidance

A future Python package should be one of:

- an external PyO3/native package wrapping `m80-firecracker`
- an external pure-Python package shelling out to `m80 run`
- an adapter-owned Python SDK wrapping adapter semantics

It should not be `crates/m80-py` inside this repository during v0.x. Keeping it
external prevents Python packaging and agent-product assumptions from becoming
part of the VM-mechanics core.

## Generic Types To Expose

Bindings should expose generic mechanics:

- command/program and argv
- cwd, env, stdin, timeout
- stdout, stderr, exit code, timing
- PTY input/output/resize/cancel
- guest file path operations
- workspace path, writeback mode, egress mode
- request id as opaque diagnostics correlation
- typed m80 error classes where practical

Bindings should not expose these as m80-core concepts:

- tool-call ids
- idempotency keys
- effect classes
- workspace authority leases
- semantic agent event enums
- product billing, placement, or registry state

## Test Expectations

External bindings should have their own packaging and smoke tests. m80 should
only add tests when the binding need reveals a missing generic invariant in the
Rust or CLI surface.

Good m80-side tests:

- direct file operation round trips
- CLI stdout/stderr/exit passthrough
- PTY byte forwarding
- warm lease allocation behavior
- typed error mapping

Poor m80-side tests:

- Python wheel packaging
- SDK class names
- agent tool names
- semantic event schemas
- adapter retry or idempotency policy

## Release Boundary

m80 release artifacts can be consumed by external bindings, but m80 releases do
not promise language-specific packages. A binding release should declare the
m80 crate/binary version it supports and run its own compatibility suite.
