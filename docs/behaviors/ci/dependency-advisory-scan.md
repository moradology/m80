# Dependency Advisory Scan

Behavior capture for beads `m80-8emae.13` and `m80-s3r28.1`.

## Contract

CI runs supply-chain checks independently from the workspace build/test job.
The audit job reads `Cargo.lock` and fails on known vulnerability advisories,
license-policy violations, denied crates, unknown sources, or unexpected
dependency-policy drift.

The scan is intentionally separate from `cargo test --workspace`:

- dependency advisories can fail fast without compiling m80
- build/test failures do not hide advisory failures
- advisory database fetches do not perturb the normal test cache key

The `audit` job installs both tools on Rust 1.85:

- `cargo deny check advisories bans licenses sources`
- `cargo audit`

`deny.toml` is the repository-owned policy. It allows Apache-2.0, MIT,
BSD-2-Clause, BSD-3-Clause, ISC, and Unicode-3.0 licenses; warns on duplicate
crate versions; denies `openssl`; and keeps sources constrained to crates.io by
default. The existing `tun` crate's WTFPL license is allowed as a crate-local
exception only, not workspace-wide.

`RUSTSEC-2024-0436` (`paste` unmaintained) is ignored in cargo-deny because it
is pulled through `rtnetlink` as a compile-time macro dependency. The ignore
reason carries a revisit date of 2026-09-01. `cargo audit` still reports the
warning so the audit log keeps visibility into the exception.

## Nix Version Follow-Up

The audit found four `nix` versions in the resolved dependency graph. This is
currently a documented upstream-blocker state, not an accidental workspace
drift:

```text
nix 0.28.0 via portable-pty
nix 0.29.0 via m80 workspace dependencies and tun
nix 0.30.1 via rtnetlink
nix 0.31.2 via vsock
```

The blockers are:

- `portable-pty 0.9.0` is the latest published `portable-pty` crate and depends
  on `nix 0.28`.
- `rtnetlink 0.21.0` is the latest published `rtnetlink` crate and depends on
  `nix 0.30.0`.
- `vsock 0.5.4` is the latest published `vsock` crate and depends on
  `nix 0.31.2`.
- `tun 0.7.13` already shares the workspace's `nix 0.29` line. Later tested
  `tun` releases (`0.7.17` through `0.8.9`) require Edition 2024 and fail to
  parse on the workspace Rust 1.82 toolchain.

Do not use a `[patch.crates-io]` override to force one `nix` version across
these crates without source-verifying each upstream call site. `nix` is still
pre-1.0 and minor releases carry API movement; a resolver trick would only hide
the dependency-policy problem.

## Verification

- `cargo audit`
- `cargo +1.85 deny check advisories bans licenses sources`
- `cargo tree -i nix@0.28.0`
- `cargo tree -i nix@0.29.0`
- `cargo tree -i nix@0.30.1`
- `cargo tree -i nix@0.31.2`
- `cargo search portable-pty --limit 3`
- `cargo search rtnetlink --limit 3`
- `cargo search vsock --limit 3`
- `cargo search tun --limit 20`
- `cargo info tun@0.7.17` and `cargo info tun@0.8.9` fail on Rust 1.82 with
  `feature edition2024 is required`
- CI workflow YAML includes an `audit` job that installs `cargo-deny` and
  `cargo-audit`, then runs both tools.
