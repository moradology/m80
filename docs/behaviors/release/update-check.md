# Update Check

Behavior beads: `m80-o3uh9.16.18.1`, `m80-o3uh9.16.9.5`,
`m80-o3uh9.16.9.4`, `m80-o3uh9.16.9.2`, `m80-o3uh9.21.9.3`.

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
`active_kind`, `freshness_state`, `latest_status_source`, `latest_status_origin`,
`latest_status_cache_state`, `latest_status_fetched_at`,
`latest_status_max_age_seconds`, `latest_status_offline_reason`,
`safety_state`, `safety_floor.status`, `safety_floor.replacement_command`,
`safety_floor.metadata_source`, `proof_cache_status`,
`proof_cache_age_seconds`, `apply_command`, `reinstall_command`, and
`retry_command`. The
`latest_status_fetched_at` value is the status artifact's `published_at` value
as Unix seconds, and the max-age policy is `172800` seconds. When emitted, the
apply command is exact, pinned, and copy-ready: safe outdated installs target the
latest stable release; unsafe or yanked state uses the safety policy's pinned `replacement_command`.
The mutating `m80 update --apply` transaction is a later leaf and must reuse the
verified installer path.

For an active `v1.2.3` install with safe latest `v1.2.4`, human output includes
one copy-ready command:

```text
apply_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh
next_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh
```

If the active tag is prerelease-shaped, build-metadata-shaped, local-dev, or
otherwise ineligible, no stable install command is guessed. If latest metadata
is unavailable, the next command is the read-only retry:

```text
retry_command=m80 update --check
next_command=m80 update --check
```

The `active_kind` field names why automatic update state may be limited:
`stable_release`, `prerelease`, `ineligible`, `local_dev`, `missing_active`, or
`stale_active_metadata`. The `freshness_state` field mirrors the finite update
state so scripts can read freshness separately from the active-install
classifier.

Safety-floor input is part of the freshness status schema. Empty policy is
explicit (`minimum_safe_tag: null`, `yanked_releases: []`) so consumers can
distinguish "publisher said no floor" from "old/malformed metadata." The floor
is advisory release metadata unless a separate release policy or CI gate names
the status artifact as release-blocking. There is no hidden `release_blocking`
field in this schema and `m80 update --check` does not infer one. It reports
and repairs from the metadata; it does not stop `m80 run` and it never updates
an old install by itself. Release promotion dispositions are tracked separately,
starting with
[`freshness-failure-policy.md`](freshness-failure-policy.md).

When present, `minimum_safe_tag.tag` and every `yanked_releases[].tag` must be
stable `vMAJOR.MINOR.PATCH` tags with a reason and advisory URL or issue id. A
yanked active or latest tag reports `yanked`; an active or latest tag below the
minimum safe tag reports `unsafe`. The safety policy's replacement commands
must be pinned install commands generated from safety metadata, never mutable
latest commands:

```text
safety_floor_replacement_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh
next_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.0/install.sh | sudo sh
```

Human output prints `safety_state` and `safety_floor_status` separately from the
overall update state so automation does not have to infer policy from prose.
Unsafe and yanked output includes the policy tag, reason, advisory or issue
reference, policy timestamp, metadata source, and pinned
`safety_floor_replacement_command`. Stale latest-status metadata reports
`safety_state=stale_metadata` and prints only `retry_command=m80 update --check`;
malformed safety policy fails closed before normal update output.

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

## Freshness Fixture Harness

Update-check behavior tests use the fake HTTP server in
`crates/m80-cli/src/cmds/update/tests/freshness_fixture_harness.rs` for
network-shaped freshness cases. Fixture routes are named
`/freshness/<scenario>.json`, where `<scenario>` names the modeled state and
the relevant tag movement, for example `current-stable-v1.2.3`,
`outdated-stable-v1.2.3-to-v1.2.4-with-floor`, `stale-status-v1.2.4`,
`malformed-missing-resolved-tag`, and `offline-status-v1.2.4`.

Add a new freshness state by adding a named fixture route, serving the status
artifact through `latest_status_input`, and asserting the rendered
`UpdateCheckState`, `active_kind`, `latest_status_origin`,
`latest_status_cache_state`, and next command. Do not point tests at GitHub or
other public URLs. The harness installs a temporary loopback-only `curl` wrapper,
only exposes `127.0.0.1` URLs, rewrites every status-advertised asset URL to
`/public-asset-trap/`, and asserts every served request path stays under
`/freshness/`; safety replacement commands may still name public release install
URLs, but they must never be fetched by `m80 update --check` tests.
