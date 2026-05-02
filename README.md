# m80 — Firecracker sandbox extraction dossier

This directory captures a detailed feasibility study for extracting Firecracker
VM sandboxing out of `predecessor` and into a focused, standalone repository
(working name: **m80**) that ships:

1. A reusable Rust library for boot/control/teardown of Firecracker microVMs
2. A no-frills CLI for spawning, exec-ing into, and tearing down sandboxes
3. The image-build pipeline (kernel discovery, rootfs preparation, provenance
   manifests) needed to make the library *runnable* on a fresh Linux host

The study is grounded in a top-to-bottom read of the relevant crates and infra
in `/tank/projects/predecessor` as of 2026-05-02.

## Files

| File | What's in it |
|---|---|
| `00-verdict.md` | Bottom-line recommendation, cost estimate, risk register |
| `01-coupling-audit.md` | What in `agent-sandbox-firecracker` is predecessor-specific vs. generic |
| `02-sandbox-api-and-guest-proto.md` | The abstraction layer: `SandboxBackend` trait, `GuestRequest`/`GuestResponse`, tool catalog |
| `03-guest-daemon.md` | What `guestd-rs` actually does inside the VM, and the cost of a generic-exec rewrite |
| `04-infra-and-artifacts.md` | Kernel, rootfs, jailer, manifests, preflight, dev-env scripts, Lima |
| `05-consumers-and-integration-seams.md` | Every predecessor consumer of the firecracker crate and what re-stubbing costs |
| `06-network-internals.md` | The 3,669-line `network.rs` module mapped end-to-end |
| `07-modules-essential-vs-hygiene.md` | Module-by-module classification (must-have / hygiene / drop) |
| `08-extraction-plan.md` | Phased plan with concrete file moves and order of operations |
| `09-cli-shape.md` | CLI surface design sketch with subcommand semantics |
| `10-risks-and-open-questions.md` | Things that will bite, things that need decisions |
| `11-loc-and-surface-budget.md` | Line counts, public surface, dependency closure |

## Headline numbers

- Source crate (`crates/sandbox/agent-sandbox-firecracker`): **27,623 LOC** across
  25 source files
- "Crystalline core" worth keeping for v0: **~15,000 LOC**
- Dead/reserved code safe to drop: **~2,400 LOC** (`snapshot.rs` 997,
  `blank_pool.rs` 1,412)
- Optional observability tail (feature-gate or split out): **~2,000 LOC**
- Net library surface after extraction: **~12,000–15,000 LOC**
- Estimated effort: **4–6 weeks** for one focused engineer to ship a
  publishable v0.1 with CLI

## How to read this dossier

Start with `00-verdict.md`. If you want the *why* behind a specific claim,
each subsequent file drills into one slice with file:line citations from the
predecessor codebase.
