# Measurement-shaped bead template

For beads whose intent is to produce a numeric result. Pair with the
`requires-verified-close` label (see ADR 0002 from `m80-lovpn.3`).

## Title

`<verb> <observable> on <substrate>` — e.g. "Measure P99 cold-launch on
minimal/idle/N=1000". Not "Add tail-latency bench harness".

## Body skeleton

### Why

(one paragraph — what decision this number unblocks)

### Observable

- **Artifact**: <path to committed file, e.g. `crates/m80-firecracker/benches/baseline.json`>
- **Field**: <JSON pointer or column, e.g. `data.wallclock.minimal.idle.p99`>
- **Substrate**: <e.g. "KVM host, kernel >= 6.5, /dev/kvm rw, sudo, dropped page cache between runs">
- **Bound** (if any): <e.g. "p99 <= 1800 ms; outliers <= 3% of N">
- **N**: <sample count required to call it statistically meaningful>

### Procedure

<reproducible command line + env vars to produce the artifact>

### Acceptance

- [ ] Artifact committed at the named path, contents non-mock.
- [ ] Observable field present and within bound.
- [ ] Reproduction command line committed under `docs/perf/<area>.md` or a
      per-bench README.
- [ ] Close reason: `verified: <artifact-path> @ <commit-sha>`.

### Not acceptance

- "Harness compiles."
- "Compute layer unit tests pass." Those are scaffolding beads, separate.
- "Mock run produces the right shape." Mocks do not satisfy this template.
