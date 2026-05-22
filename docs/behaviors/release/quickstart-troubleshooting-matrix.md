# Quickstart Troubleshooting Matrix

The public quickstart uses stable failure IDs so support reports, docs, and
fixtures talk about the same first-run problem. The IDs below are stable enough
for bug reports. Rename one only with a replacement row and release-note
migration.

The machine source of truth is
[`quickstart-troubleshooting-matrix.json`](quickstart-troubleshooting-matrix.json),
validated by `scripts/verify-quickstart-troubleshooting.py`.

## network

Network, DNS, or HTTP failure. Retry after checking GitHub reachability:
`curl -I https://github.com/moradology/m80/releases/latest`.

## github-auth-rate-limit

GitHub auth, verifier, or rate-limit failure. Check `gh auth status` and install
or upgrade GitHub CLI with attestation support when needed.

## missing-asset

A required public release asset is missing. Treat this as a release bug unless
the release notes say the asset moved; use a pinned install only after the
release is repaired.

## checksum-provenance-mismatch

Checksum, provenance, or release-integrity material did not agree. Do not bypass
verification. Capture `m80 install-status` and report the release.

## unsupported-tuple

The current Linux/architecture/image tuple is unsupported by the public release.
Capture `uname -s && uname -m` and the asset-index diagnostic.

## missing-local-tool

The installer could not find a required local tool. Check:
`command -v curl python3 sha256sum tar mktemp chmod gh`.

## kvm-unavailable

`/dev/kvm` is missing or not writable. Run `m80 preflight` and repair host KVM
access before trying to launch a VM.

## host-prerequisite

Firecracker, jailer, seccomp, cgroup, kernel, or host TCB prerequisite failed.
Run `m80 preflight`; the structured remediation fields point to the exact host
setup policy or repair command.

## privilege-denied

The current user lacks the privilege or capability set required for the selected
launch path. Run `m80 preflight` and compare the result with
[`docs/behaviors/preflight/privilege.md`](../preflight/privilege.md).

## stale-profile

The active pointer, default profile, installed metadata, or proof cache is
stale. Start with `m80 install-status`; reinstall the active or desired pinned
release when status says the local install is unhealthy.

## process-smoke-failed

Install completed, but `m80 run -- echo hello` did not return `hello`. Run
`m80 preflight` and inspect `m80 logs <vm-id>` when a VM id was created.

## unknown-report

The failure does not fit a known row. Capture `m80 --json env > m80-env.json`
and include the first failing command, exit status, and bounded stderr excerpt.
