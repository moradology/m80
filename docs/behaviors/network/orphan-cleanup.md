# Network Orphan Cleanup

`m80 net cleanup` scans host network residue that no longer has a usable
per-VM `network-state.json` file.

The command inspects `iptables -S` output for m80 rule comments of the form
`m80:<run-root-digest>:<tap-name>` and `ip -o link show` output for TAP links
named `tfc<12-hex>`. Existing run-root state files are treated as ownership
evidence and are kept. Matching rules without state are reported as orphans.
TAP names alone do not encode the run root, so a TAP without state is deleted
only when the selected run root owns it through either a derived current run-dir
candidate or a matching orphaned m80 iptables comment. Unattributed `tfc*` TAPs
are reported and kept.

`m80 net cleanup --dry-run` prints the same structured rows but does not delete
anything. Without `--dry-run`, orphan iptables rules are deleted by replaying
the `iptables -D` form of the discovered rule, empty `tfw<12-hex>` filter
chains are deleted after their rules are removed, and attributed orphan TAP
links are deleted with `ip link delete <tap>`.

Human output is a table with `ORPHAN_VM_ID`, `RESOURCE`, `ACTION`, and `TEXT`.
JSON output uses the normal m80 envelope and includes the same resource rows.
When a deleted run directory means the original VM id cannot be recovered from
the comment tag, `orphan_vm_id` is `unknown`.

## Coverage

- Unit: `crates/m80-cli/src/cmds/net/tests.rs`
- CLI subprocess: `crates/m80-cli/tests/network/orphan_cleanup.rs`
