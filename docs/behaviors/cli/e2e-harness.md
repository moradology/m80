# CLI E2E Harness

Behavior capture for bead `m80-lt15.8`.

## Default CI Path

The default CLI smoke path is intentionally non-KVM:

```text
./scripts/smoke-cli.sh
```

It builds `m80-cli` and runs `cargo test -p m80-cli --tests`. The ignored KVM
tests remain compiled by the normal test target but are not executed unless the
operator opts in.

## KVM Path

Run the full facade smoke on a KVM-capable host with built m80 artifacts:

```text
sudo M80_RUN_KVM_E2E=1 \
  M80_FIRECRACKER_BIN=/path/to/firecracker \
  M80_JAILER_BIN=/path/to/jailer \
  M80_JAILER_HARDEN_BIN=/path/to/m80-jailer-harden \
  M80_KERNEL_IMAGE=/path/to/vmlinux \
  M80_ROOTFS_IMAGE=/path/to/rootfs.ext4 \
  ./scripts/smoke-cli.sh
```

Optional inputs:

- `M80_RUN_ROOT` - run-root for smoke state; defaults to a temporary directory
- `M80_BIN` - m80 binary to smoke; defaults to `target/debug/m80`
- `M80_CGROUP_MODE` - defaults to `disabled` for local smoke
- `M80_MAX_CONCURRENT_VMS` - defaults to `4`
- `M80_SMOKE_KEEP_OUTPUT=1` - keep captured stdout/stderr files after the run

## Smoke Matrix

`scripts/smoke-cli.sh` exits on the first failure. In KVM mode it covers:

- `m80 version`, top-level help, `m80 run --help`, and `m80 warm --help`
- `m80 preflight` and `m80 config show`
- profile selection through `m80 run --profile env`
- transparent stdout/stderr and guest exit-code preservation
- workspace visibility, explicit scratch sizing, and `--writeback always`
- real-time pipe streaming: `early` is observed before the guest process exits
- explicit `--egress none` and `--egress outbound` policy selection
- run-root debug commands: `list`, expected-failure `inspect`, and `cleanup`
- JSON error envelope for a wrapper config failure
- `m80 run --warm` with no owner, proving no cold fallback
- ignored PTY e2e: `crates/m80-cli/tests/e2e_tty.rs`
- ignored warm-owner e2e: `crates/m80-cli/tests/e2e_warm.rs`, including a
  warm streaming probe that observes `early` before the child exits

On failure, the script prints `M80_RUN_ROOT` and tails available `state.json`,
`diagnostics.jsonl`, `console.log`, and `owner.json` files. Those files are the
triage path for request ids, run directories, owner identity, and guest logs.

## Boundaries

The smoke script does not build rootfs images; use the image-build pipeline or
an existing profile/artifact setup first. The script also does not contact an
external service. The outbound egress check verifies that the policy reaches the
sandbox setup path without requiring public network access.
