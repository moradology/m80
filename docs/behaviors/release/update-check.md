# Update Check

Behavior beads: `m80-o3uh9.16.18.1`, `m80-o3uh9.16.9.4`,
`m80-o3uh9.16.9.2`.

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
and never refreshes the proof cache or latest-status cache. If no freshness status
artifact can be fetched and no usable cache is available, the result is
`unknown_offline`; the command does not guess that the active release is current.
Malformed freshness metadata fails closed for the selected source. Malformed
fallback-cache metadata is reported as `unknown_offline` with
`latest_status_cache_state: "malformed"` because the cache cannot be trusted.
The only permitted remote read in check mode is the selected latest-status
artifact (`--latest-status-url` or the default public freshness proof asset);
bundle, selector, checksum, proof, and install assets are apply/install inputs,
not check inputs.

JSON output uses schema `1` and includes `active_tag`, `latest_stable_tag`,
`latest_status_source`, `latest_status_origin`,
`latest_status_cache_state`, `latest_status_fetched_at`,
`latest_status_max_age_seconds`, `latest_status_offline_reason`,
`safety_floor.status`, `proof_cache_status`, `proof_cache_age_seconds`,
`apply_command`, `reinstall_command`, and `retry_command`. The
`latest_status_fetched_at` value is the status artifact's `published_at` value
as Unix seconds, and the max-age policy is `172800` seconds. When emitted, the
apply command is the exact pinned release installer for the latest stable safe
target; no apply command is emitted when the latest target is yanked or below
the safety floor. The mutating `m80 update --apply` transaction is a later leaf
and must reuse the verified installer path.

Safety-floor input is optional until the freshness status publisher grows the
public policy artifact. When present, `minimum_safe_tag` and `yanked_tags` must
be stable `vMAJOR.MINOR.PATCH` tags. A yanked active or latest tag reports
`yanked`; an active or latest tag below the minimum safe tag reports `unsafe`.

The default latest-status source is the public release asset
`m80-latest-freshness-proof.json` under the GitHub latest release. Tests may pass
`--latest-status <path>` to stay network-free. Operators may pass both
`--latest-status-url <url>` and `--latest-status <path>`; the URL is tried first,
and the path is a read-only fallback cache if the URL is unavailable. A fresh
fallback cache can still classify `current` or `outdated` while carrying
`latest_status_offline_reason`. An expired fallback cache reports
`stale_latest_metadata`, names `latest_status_cache_state: "stale"`, and prints
`retry_command=m80 update --check`. A missing fallback cache reports
`latest_status_cache_state: "missing"` and leaves the installed version's
freshness unknown.
