# CPU Vulnerability Preflight

Behavior capture for bead `m80-8emae.9`.

## Contract

`m80-preflight` reads selected Linux kernel status files under
`/sys/devices/system/cpu/vulnerabilities/` before binary discovery and artifact
validation.

Hard-gated files:

- `mds`
- `l1tf`

If either hard-gated file starts with `Vulnerable`, preflight returns
`PreflightError::CpuVulnerabilityDetected { id, detail }`. The `detail` field
preserves the kernel-provided status text with whitespace collapsed to one line.

Advisory files:

- `spectre_v2`
- `retbleed`
- `tsx_async_abort`
- `srbds`
- `mmio_stale_data`
- `gather_data_sampling`

For advisory files, `Vulnerable`, unavailable, unreadable, and unclassified
statuses do not fail preflight. They are retained in the `CPU vulnerabilities`
report row so operators can see the host's exact side-channel posture.

`M80_SKIP_CHECK_VULNERABILITIES=1` skips the hard gate and emits a report row
recording the skip. Other values do not disable the gate.

## Non-Contract

This check does not interpret CPU vendor, microcode revision, SMT status, or
workload placement policy. It only surfaces the kernel's vulnerability status
files and hard-fails the two highest-impact rows m80 currently treats as
required mitigations.

## Verification

- `crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_mds_vulnerable_fails_closed`
- `crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_medium_vulnerable_is_advisory_row`
- `crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_scan_reports_all_configured_files`
- `crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_scan_can_be_explicitly_skipped`
- `crates/m80-preflight/tests/preflight/cpu_vulnerabilities.rs::cpu_vulnerability_error_names_status_file_and_kernel_text`
