# Privileged E2E Reaper

`scripts/e2e-reap.sh` removes stale m80-owned host residue before a privileged
test session. `scripts/run-e2e.sh` invokes it automatically before real runs.

## What It Catches

- Host links whose names match m80's deterministic network shapes:
  `tfc[0-9a-f]{12}`, `brfc[0-9a-f]{11}`, `bfc[0-9a-f]{12}`, and legacy
  `m80-br*`.
- iptables rules in `filter` and `nat` tables that carry an m80 rule comment
  beginning with `m80:`.
- Empty per-VM iptables chains named `tfw[0-9a-f]{12}`. A chain with any
  foreign rule is preserved.
- Direct child run directories under `/var/lib/m80-r`, `/var/lib/m80-run`, or
  explicit safe m80 run roots when they are older than the configured age and
  contain no live `*.pid` process.
- Empty cgroup v2 leaves under `/sys/fs/cgroup/m80-firecracker`.

## What It Does Not Catch

- Foreign links, chains, or rules that do not carry m80 names/comments.
- Run directories younger than the age threshold.
- Run directories whose pid files still point at live processes.
- Preserved failure evidence under `.preserved`, warm-owner state under `warm`,
  and template/cache state under `templates`.
- Ambiguous iptables chains that contain foreign rules.

## Commands

Inspect without mutating:

```sh
scripts/e2e-reap.sh --dry-run --json | jq '.actions, .skipped, .errors'
```

Run cleanup for the default E2E root:

```sh
sudo -n scripts/e2e-reap.sh --run-root /var/lib/m80-r
```

Adjust the run-dir age threshold:

```sh
sudo -n scripts/e2e-reap.sh --run-root /var/lib/m80-r --min-age-hours 6
```

`scripts/run-e2e.sh` uses `M80_E2E_REAP_MIN_AGE_HOURS`, defaulting to `1`.
Set `M80_E2E_SKIP_REAPER=1` only while debugging stale state by hand.

## Safety Boundary

The reaper deletes by exact m80 ownership signals: deterministic link names,
`m80:` iptables comments, per-VM `tfw` chain names, safe m80 run-root prefixes,
and empty m80 cgroup leaves. It refuses broad run roots such as `/`, `/tmp`,
`/var`, and `/tank/tmp`.
