# draft-release-notes Smoke

Date: 2026-05-27

Invocation under test:

```text
/draft-release-notes v0.2.24..v0.2.25
```

Execution mode: Codex manually followed
`.claude/skills/draft-release-notes/SKILL.md`. `claude --help` on this host
reports that skills resolve by slash name, and the skill is installed at the
project-level Claude Code path.

## Evidence Collected

```text
$ git log --no-merges --format='%h %s%n%b' v0.2.24..v0.2.25
8e37e611 Prepare v0.2.25 release

77eb2c7b Mark v0.2.24 public installer fresh
```

```text
$ git diff --stat v0.2.24..v0.2.25
16 files changed, 258 insertions(+), 300 deletions(-)
```

```text
$ br changelog --since-tag v0.2.24 --json
{
  "since": "v0.2.24",
  "total_closed": 2,
  "groups": [
    {
      "issue_type": "task",
      "label": "Tasks",
      "issues": [
        {
          "id": "m80-6egdo.3",
          "title": "Wire release-artifacts.yml: extract CHANGELOG section into Release body",
          "priority": "P2",
          "closed_at": "2026-05-27T13:36:38.727342593+00:00"
        },
        {
          "id": "m80-6egdo.1",
          "title": "Investigate br changelog usability for the release-notes pipeline",
          "priority": "P2",
          "closed_at": "2026-05-27T13:36:38.194048666+00:00"
        }
      ]
    }
  ]
}
```

The `br changelog` output was ignored for draft content because those beads were
closed after `v0.2.25` and are not in `git log v0.2.24..v0.2.25`.

## Draft Output

Draft suggestion — human review required

## [v0.2.25] — 2026-05-27

v0.2.25 carries the proved current-latest release state forward and keeps
privileged measurement benchmarks out of ordinary CI/test builds.

### Changed

- Marked the public installer freshness proof green for `v0.2.24` in the README
  and release runbook, including unauthenticated latest/pinned install URL
  evidence.
- Advanced the current-latest repair source contract to workspace version
  `0.2.25`, with `v0.2.24` treated as the existing public latest tag for repair
  preflight ordering checks.
- Gated real-KVM Cargo benchmark binaries behind the `real-kvm-bench` feature
  so `cargo test --workspace --all-targets` does not build privileged
  measurement programs unless explicitly requested.

### Fixed

- Updated snapshot-template benchmark documentation and verifier expectations so
  real-KVM benchmark reproduction commands include `--features real-kvm-bench`.

Consider also:

- `Prepare v0.2.25 release` includes the mechanical workspace version bump; a
  human reviewer may choose to omit that from final release notes.

Reviewer checklist:

- [ ] Checked each bullet against the commit range.
- [ ] Removed speculative, duplicated, or purely internal bullets.
- [ ] Verified user/operator-facing wording.
- [ ] Edited the final text into CHANGELOG.md manually.
