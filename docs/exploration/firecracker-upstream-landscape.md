# Firecracker Upstream Landscape For Jailer Contributions

Date: 2026-05-27

Scope: determine whether an upstream Firecracker jailer patch is realistic and
what shape would have the best chance. Source pin for repository policy files:
Firecracker v1.15.1, `f82c0bd0f0a74015642a0d452880f3ad10147b14`. GitHub PR
state was queried live on 2026-05-27.

## Maintainer Surface

Firecracker's checked-in `CODEOWNERS` does not name a specific owner for
`src/jailer/`. It only assigns broad ownership for markdown, selected docs, root
metadata, license, notice, and PGP key files
([CODEOWNERS#L1-L19](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/.github/CODEOWNERS#L1-L19)).

The practical jailer reviewer surface is therefore inferred from recent merged
jailer PRs, not from a path-specific ownership file. Recent approvals repeatedly
include `Manciukic`, with `ShadowCurse`, `kalyazin`, `xmarcalx`, `roypat`, and
`zulinx86` appearing on representative jailer changes.

## Contribution Process

The official process is fork-and-pull. A contributor opens a PR against `main`,
works with reviewers, and needs at least two maintainer approvals before merge
([CONTRIBUTING.md#L21-L40](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/CONTRIBUTING.md#L21-L40)).
For proposal-level feedback, Firecracker explicitly supports an `[RFC]` PR path
([CONTRIBUTING.md#L41-L55](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/CONTRIBUTING.md#L41-L55)).

Quality gates matter for a jailer patch:

- Run `tools/devtool checkstyle` and `tools/devtool checkbuild --all`
  ([CONTRIBUTING.md#L57-L75](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/CONTRIBUTING.md#L57-L75),
  [pull_request_template.md#L16-L32](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/.github/pull_request_template.md#L16-L32)).
- Keep logical changes separated, increase coverage, and include integration
  tests for new functionality
  ([CONTRIBUTING.md#L77-L89](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/CONTRIBUTING.md#L77-L89)).
- Use DCO signoff on commits
  ([CONTRIBUTING.md#L161-L178](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/CONTRIBUTING.md#L161-L178)).
- The PR template asks whether the functionality belongs in `rust-vmm`
  ([pull_request_template.md#L35-L40](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/.github/pull_request_template.md#L35-L40)).

Security reports are private through AWS/Amazon Security, not public GitHub
issues
([SECURITY.md#L1-L9](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/SECURITY.md#L1-L9)).
A final-exec hardening improvement that does not disclose a live vulnerability
can be public; a concrete exploitable finding should not be.

## Representative Jailer Work

| Item | State | Timeline | Files touched | Signal |
|---|---|---:|---|---|
| [PR #5811: use `O_NOFOLLOW` for cgroup and netns file operations](https://github.com/firecracker-microvm/firecracker/pull/5811) | Merged | 2026-04-02 to 2026-04-07 | `src/jailer/src/env.rs`, `src/jailer/src/main.rs` | Security-shaped, narrow jailer hardening can merge in about a week with two maintainer approvals. |
| [PR #5631: update binary copy function](https://github.com/firecracker-microvm/firecracker/pull/5631) | Merged | 2026-01-15 to 2026-01-16 | `CHANGELOG.md`, jailer source, `tests/integration_tests/security/test_jail.py` | Small bug/security behavior with integration coverage can merge quickly. |
| [PR #5431: clarify parent cgroup behavior](https://github.com/firecracker-microvm/firecracker/pull/5431) | Merged | 2025-09-08 to 2025-09-10 | `docs/jailer.md`, jailer source, security integration test | Behavior clarification plus test is accepted when scoped to existing jailer semantics. |
| [PR #5282: measure jailer startup performance](https://github.com/firecracker-microvm/firecracker/pull/5282) | Merged | 2025-06-26 to 2025-07-02 | performance pipeline, jailer source, performance and security tests | Non-trivial jailer changes still move if the measurement/test story is clear. |
| [PR #5771: add Landlock LSM sandboxing via `--landlock`](https://github.com/firecracker-microvm/firecracker/pull/5771) | Open | opened 2026-03-18 | broad docs, jailer source, new module, tests | Broad sandboxing surface can stall even when the idea is security-positive. |
| [Issue #5513: use Landlock for sandboxing VM process](https://github.com/firecracker-microvm/firecracker/issues/5513) | Open, parked | opened 2025-11-17 | n/a | New LSM-level sandboxing is not obviously on the near-term path. |
| [Issue #5587: `mknod_and_own_dev` and group permissions](https://github.com/firecracker-microvm/firecracker/issues/5587) | Closed awaiting author | 2025-12-17 to 2026-02-11 | n/a | Upstream will close underspecified or inactive jailer requests. |

## Recommendation

An upstream PR is realistic only if it is narrow. The best path is an RFC or
small draft PR that changes the official jailer final-exec boundary, includes
security integration tests, and does not try to add a new general sandboxing
subsystem. The proposal should explicitly say this is not a replacement for
Firecracker's VMM seccomp, not an m80-specific adapter feature, and not a
Landlock/AppArmor/SELinux policy system.

Do not plan for a fork first. Plan for upstream first, with an explicit fallback
to accepting the documented residual if upstream rejects or stalls. A downstream
patch is technically possible, but the maintenance burden is not justified by
the current finite gap unless a future audit turns it into a concrete exploitable
host-risk finding.
