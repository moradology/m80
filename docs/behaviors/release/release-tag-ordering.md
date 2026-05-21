# Release Tag Ordering

Behavior bead: `m80-o3uh9.16.12.1`.

Automatic install and update paths only compare concrete stable release tags.
The eligible tag shape is exactly `vMAJOR.MINOR.PATCH`; each component is a
decimal integer and the ordering key is `(MAJOR, MINOR, PATCH)`.

The ordering policy reports one of these finite transition states:

- `upgrade_allowed`: the target stable tag is newer than the active stable tag.
- `already_current`: the target and active stable tags are equal.
- `downgrade_refused`: the target stable tag is older than the active stable
  tag. Automatic update paths must not move active state to this target; a
  future rollback command must be explicit.
- `active_prerelease`: the active identity has a prerelease suffix such as
  `v1.2.3-rc.1`.
- `target_prerelease`: the requested target has a prerelease suffix.
- `active_build_metadata`: the active identity uses build metadata such as
  `v1.2.3+build.1`.
- `target_build_metadata`: the requested target uses build metadata.
- `active_malformed`: the active identity is neither stable nor a recognized
  prerelease/build-metadata tag.
- `target_malformed`: the target identity is malformed, including mutable
  aliases such as `latest`.
- `active_local_dev`: the active install is a local/dev tree rather than a
  release tag.

Each transition report carries the expected ordering rule, observed active tag
when one exists, observed target tag, an observed ordering label such as
`target_newer`, `target_same`, or `target_older`, and a diagnostic string that
includes those values. This gives future `m80 update --check`, upgrade, and
rollback command paths one shared way to explain why a target is accepted or
refused.

Current installer input validation uses the same stable-tag parser before it
fetches asset-index metadata. That keeps `m80 install --release-tag`,
bootstrapper handoff, official bundle URL checks, and freshness parsing aligned
on one stable-channel definition.
