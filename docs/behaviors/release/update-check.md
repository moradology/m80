# Update Check

Behavior beads: `m80-o3uh9.16.18.1`, `m80-o3uh9.16.9.4`.

`m80 update --check` is the read-only update surface. It reads local install
state, the installed proof-cache summary, and bounded latest freshness metadata,
then reports one finite state without changing the install root:

- `current`
- `outdated`
- `yanked`
- `unsafe`
- `unknown_offline`
- `stale_latest_metadata`
- `prerelease_active`
- `ineligible_active`
- `local_dev_install`
- `install_unhealthy`

The command never flips `<install-root>/active`, never rewrites profiles, never downloads a bundle,
and never refreshes the proof cache. If no freshness status
artifact can be fetched, the result is `unknown_offline`; the command does not
guess that the active release is current. Malformed freshness metadata fails closed.
The only permitted remote read in check mode is the selected latest-status
artifact (`--latest-status-url` or the default public freshness proof asset);
bundle, selector, checksum, proof, and install assets are apply/install inputs,
not check inputs.

JSON output uses schema `1` and includes `active_tag`, `latest_stable_tag`,
`safety_floor.status`, `proof_cache_status`, `proof_cache_age_seconds`,
`apply_command`, and `reinstall_command`. When emitted, the apply command is the
exact pinned release installer for the latest stable safe target; no apply
command is emitted when the latest target is yanked or below the safety floor.
The mutating `m80 update --apply` transaction is a later leaf and must reuse the
verified installer path.

Safety-floor input is optional until the freshness status publisher grows the
public policy artifact. When present, `minimum_safe_tag` and `yanked_tags` must
be stable `vMAJOR.MINOR.PATCH` tags. A yanked active or latest tag reports
`yanked`; an active or latest tag below the minimum safe tag reports `unsafe`.

The default latest-status source is the public release asset
`m80-latest-freshness-proof.json` under the GitHub latest release. Tests may pass
`--latest-status <path>` to stay network-free.
