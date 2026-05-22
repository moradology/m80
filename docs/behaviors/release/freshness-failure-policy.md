# Freshness Failure Policy

Behavior bead: `m80-o3uh9.21.2.1`.

The scheduled public freshness lane reports failures through a checked taxonomy
instead of hard-coded shell branches. The policy lives at
`docs/behaviors/release/freshness-failure-policy.json` and is validated by:

```sh
python3 scripts/verify-freshness-failure-policy.py
```

The config schema is intentionally small. Every verifier-emitted class must
have one row, and every row must name a disposition, owner, safe summary fields,
retry count, repair command, and whether it blocks the next release/latest
promotion, opens a repair bead, pages a maintainer, or requires manual operator
confirmation.

Current verifier-emitted classes:

| Failure class | Primary disposition | Operator action |
| --- | --- | --- |
| `network-transient` | `retry-only` | Retry the bounded public fetch once before filing work. |
| `stale-latest` | `block-next-release-latest` | Block release/latest movement until latest resolves to the expected stable tag. |
| `missing-public-asset` | `block-next-release-latest` | Repair or republish the public release asset set. |
| `docs-drift` | `open-update-bead` | Update the docs command inventory or generated freshness status. |
| `checksum-mismatch` | `block-next-release-latest` | Rebuild or reject the release integrity material. |
| `provenance-mismatch` | `block-next-release-latest` | Repair attestation, trust-policy, asset-index, or URL identity drift. |
| `public-release-unavailable` | `block-next-release-latest` | Restore an eligible public stable latest release before trusting the channel. |
| `real-kvm-substrate-unavailable` | `manual-operator-confirmation` | Fix the privileged KVM runner or record explicit operator confirmation. |
| `verifier-schema-drift` | `page-maintainer` | Page the release maintainer because the verifier can no longer trust its inputs. |

Adding a class is a two-part change. First teach the verifier to emit the new
class, then add exactly one row to the JSON policy with a disposition and safe
summary fields. `scripts/test-freshness-failure-policy.py` proves the config
rejects unknown classes, duplicate classes, missing or invalid dispositions,
unsafe repair commands, and any verifier-emitted class absent from the config.

The policy does not create beads or pages by itself. It is the machine-readable
input for the later freshness gate, bead-upsert, and notification leaves. Until
those consumers land, the release runbook uses this file as the authority for
manual handling.
