# CLI Template Commands

Bead: `m80-q420k.5.2`.

`m80 template` is the operator surface for committed
`m80-snapshot-template` bodies. It manages host-side template artifacts by
fingerprint. Template build runs the Phase D producer path and therefore boots,
captures, stops, and deletes a real VM when invoked with a valid BootSpec.

## Commands

```text
m80 template build <name> --boot-spec <file>
m80 template list [--store <store>]
m80 template show <fingerprint> [--store <store>]
m80 template prune [--store <store>]
m80 template rm <fingerprint> [--store <store>]
```

The default template store is `/var/lib/m80/templates`.

`build` accepts YAML or JSON BootSpec files. Parse and typed validation errors
are returned before preflight, backend construction, or VM admission. Valid
builds require `warm_strategy.mode: snapshot_restore`; the command uses the
BootSpec's template-store path and hook set, then derives the committed
fingerprint from live host inputs. The restore fingerprint inside the BootSpec
is a restore selector, not a persistent name or forced build output.

`list` reads the store index and prints committed fingerprints, byte size, and
last-used timestamp. `show` reads the committed manifest for one fingerprint.
`rm` removes one unpinned committed template and updates the index.

`prune` is scoped. With no BootSpec it opens the store and exits 0 with
`removed_count: 0`. With `--boot-spec`, the BootSpec is parsed first, live
template inputs are computed from the current host without launching a VM, and
only unpinned templates in the same conservative prune scope are removed. The
scope is the manifest's pmem image set, post-init state digest, and hook set.
That prunes host-kernel, Firecracker-version, or guest-kernel invalidations for
one family without sweeping unrelated valid templates from the same store.

## Output

Human `list` output uses this tabular shape:

```text
FINGERPRINT    SIZE_BYTES    LAST_USED_UNIX_MS
```

Human `show` output prints the fingerprint, indexed size and last-use fields
when present, manifest schema version, pmem-layer count, and hook count.

`--json` emits the standard CLI envelope. For list:

```json
{
  "version": 1,
  "data": {
    "store": "/var/lib/m80/templates",
    "total_size_bytes": 123,
    "templates": [
      {
        "fingerprint": "64 lowercase hex characters",
        "size_bytes": 123,
        "last_used_unix_ms": 1760000000000
      }
    ]
  }
}
```

`show` includes the full committed `TemplateManifest` in the envelope. `prune`
returns `removed_count` and a `mode` of either `noop_without_boot_spec` or
`scoped_boot_spec`.

## Safety

Template fingerprints are content addresses. The CLI does not persist `<name>`
as an alias and does not accept case-insensitive or shortened fingerprints.
`rm` is explicit and single-fingerprint only. `prune --boot-spec` is deliberately
conservative: it can leave stale templates from a changed pmem set or changed
post-init state behind, because deleting them safely requires stronger
template-family metadata. Operators can remove those explicitly with `rm`.

## Tests

- `crates/m80-cli/tests/parse_args.rs` pins the argv shape.
- `crates/m80-cli/tests/help_smoke.rs` proves all template subcommands render
  help.
- `crates/m80-cli/tests/template_smoke.rs` covers list, show, rm, JSON
  envelopes, human output, parse-time build rejection, and prune parse
  rejection without a KVM dependency.
- `crates/m80-snapshot-template/tests/store.rs` covers list, manifest, remove,
  pin refusal, invalidation, and index behavior at the typed store layer.
