# Logging Operations

`docs/ops/logging.md` documents that `console.log` is guest-influenced output,
not a trusted host event stream. Operators should avoid external log shipping
for that file unless the destination is approved for guest data.

m80 persists at most 2 MiB of console stdout/stderr per VM. The jailer launch
path drains bytes beyond the cap and discards them, so a noisy guest cannot fill
host disk through the console log or block Firecracker on a full pipe.

Guest boot phase markers parsed from `console.log` are constrained to
`[a-z0-9_]` before m80 formats them as host phase names.

Tests:

- `crates/m80-firecracker/src/launch/tests.rs::console_guest_boot_parser_rejects_injected_phase_names`
- `crates/m80-jailer/src/materialized_tests.rs::stdio_log_copier_caps_persisted_bytes`
- `crates/m80-jailer/src/materialized_tests.rs::stdio_log_copier_respects_existing_bytes`
- `crates/m80-jailer/src/materialized_tests.rs::stdio_log_copiers_share_one_byte_cap`
