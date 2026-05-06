# External Adapter Boundary

## Scope

This document describes how an external adapter should sit on top of m80. The
first expected consumer is a future `m80-adapter`, but the same boundary
applies to any product-specific facade.

m80 stays generic. The adapter translates product semantics into m80 mechanics.

## Ownership Table

| Concern | Owner | Notes |
|---|---|---|
| Boot artifact discovery and validation | m80 | `m80-preflight`, manifests, profiles, and launch config. |
| VM lifecycle | m80 | Launch, running, stopped, delete, warm lease, and persistent exec. |
| Process execution | m80 | Program, argv, cwd, stdin, env, timeout, stdout, stderr, exit. |
| PTY execution | m80 | Terminal bytes, resize, input, cancel, and terminal exit. |
| Guest file operations | m80 | Read, write, stat, list, remove, and chunk upload by guest path. |
| Workspace projection | m80 | One explicit host directory mounted at `/workspace`. |
| Workspace writeback mechanics | m80 | Extract requested workspace mutations according to generic caller options. |
| Egress mechanics | m80 | `none` or outbound NAT and future generic allowlist mechanics. |
| Diagnostics | m80 | Opaque `request_id`, host lifecycle events, guest stderr/console evidence. |
| Cleanup evidence | m80 | Stop/delete phases and non-release evidence about owned VM residue. |
| Tool catalog | Adapter | Maps product tool names such as `bash`, `exec`, or file tools. |
| Semantic identities | Adapter | `tool_call_id`, `workspace_id`, correlation IDs, and idempotency keys. |
| Effect class and authority | Adapter | Read-only/mutating policy, writeback authority, and commit deadlines. |
| Product events | Adapter | `sandbox_exec_*`, reset, placement, billing, and user-visible events. |
| HTTP/SDK object model | Adapter | Routes, SDK classes, retries, registry, and product-specific lifecycle names. |

## Request Translation

An adapter request should resolve into one of m80's generic operations:

- exec request: `program`, `args`, `cwd`, `env`, `stdin`, `timeout`
- PTY request: program/args plus terminal size and input stream
- file request: guest path plus operation-specific bytes or metadata
- lifecycle request: cold launch, warm lease, persistent session exec, stop,
  delete, or diagnostics inspection

The adapter may store product metadata in its own records. It should pass only
an opaque `request_id` into m80 when cross-layer correlation is useful.

## Tool Catalog Mapping

A predecessor-shaped adapter might map tools like this:

| Adapter tool | m80 operation |
|---|---|
| `bash` | `ExecRequest { program: "bash", args: ["-lc", script], ... }` |
| `exec` | `ExecRequest { program, args, ... }` |
| `read_file` | `RunningSandbox::read_file(path, max_bytes)` |
| `write_file` | `RunningSandbox::write_file(path, bytes, mode)` |
| `list_dir` | `RunningSandbox::list_dir(path)` |
| `stat_file` | `RunningSandbox::stat_file(path)` |
| `remove_file` | `RunningSandbox::remove_file(path)` |

m80 should not know that these adapter tool names exist. That lets another
adapter choose a different vocabulary without changing the VM layer.

## Workspace And Writeback Authority

m80 can expose a workspace and perform requested extraction mechanics. It does
not decide whether a product is allowed to commit those changes.

Adapter responsibilities:

- authorize the workspace before launch
- decide whether a tool call may mutate it
- choose m80's generic writeback mode
- validate the extracted result against product policy
- commit, reject, or report the result

m80 responsibilities:

- prepare the scratch/workspace image
- mount it at the documented guest path
- extract requested changes after stop
- preserve evidence when extraction or cleanup fails

## Lifecycle Choice

The adapter chooses the lifecycle based on product semantics:

- cold run: clean state, highest latency, simplest isolation
- warm stateless run: clean pre-restored slot, low launch latency
- persistent session: shared VM state across multiple execs
- PTY run: terminal-oriented process with merged output stream

m80 exposes these mechanics. It does not decide which semantic session deserves
which lifecycle.

## Error And Event Mapping

m80 errors should remain typed VM-mechanics errors. The adapter maps them into
product-facing errors and events.

Examples:

- m80 `PoolEmpty` can become an adapter capacity error.
- m80 `FileOp` can become a tool-result failure for `read_file` or
  `write_file`.
- m80 cancellation can become a product timeout, user abort, or lease-revoked
  event depending on the adapter's cause.

The inverse should not happen: m80 should not emit adapter event names or carry
adapter-only cause enums.

## Repository Boundary

This repository should not add:

- `crates/m80-adapter`
- `crates/m80-py`
- `m80 agent ...`
- in-core tool catalog schemas
- agent semantic event enums
- idempotency or authority state machines

If those surfaces are needed, create them in the adapter package. Changes in
m80 should be limited to generic mechanics the adapter needs, such as a new
file operation, lifecycle primitive, diagnostics field, or CLI/process wrapper
flag that is also useful outside the agent product.

## Acceptance Checklist For Future Adapter Work

Before adding a feature to m80 for an adapter, ask:

1. Can a non-agent caller use this as a VM/process/filesystem/network mechanic?
2. Does the public type avoid semantic product IDs and product event names?
3. Is the same behavior testable without a tool catalog or authority service?
4. Does the owning crate README describe the invariant as generic m80 behavior?
5. Would two different adapters be able to put different product semantics on
   top of the same primitive?

If the answer to any question is no, the feature belongs above m80.
