# CLI Image Commands

Bead: `m80-q420k.5.1`.

`m80 image` is the operator surface for the content-addressed
`m80-image-store`. It manages host-side image artifacts only. It does not boot a
VM, attach pmem, mount erofs in the guest, or build snapshot templates.

## Commands

```text
m80 image build <name> --source <path> --out <store> [--kind erofs|ext4]
m80 image gc [--store <store>] [--template-store <store>] [--keep <digest>...]
             [--pin-file <path>] [--min-age <duration>] [--execute]
m80 image list [--store <store>]
m80 image show <digest> [--store <store>]
m80 image verify <digest> [--store <store>]
m80 image rm <digest> [--store <store>] [--template-store <store>]
```

The default image store is `/var/lib/m80-images`. The default template store
checked by `rm` is `/var/lib/m80/templates`.

Images remain content-addressed. `<name>` is an operator label echoed in build
output; it is not stored as an alias and cannot be used instead of the digest.

`build` has two source modes:

- directory source: calls `ImageStore::build_minimal_test_image`;
- regular file source: calls `ImageStore::import_existing`.

The build helper is intentionally the small local-dev/test builder from
`m80-image-store`, not a production image pipeline. Production builders feed a
completed `.erofs` or `.ext4` file into the same import path.

## GC

`m80 image gc` is report-only by default. It walks the selected image store,
groups artifacts by digest, and renders each digest as either:

- `candidate`: no protection reason was found, so `--execute` would try to
  remove every artifact kind for the digest;
- `protected`: at least one retention reason was found;
- `removed`: the digest was a candidate and `--execute` removed it.

The protection reasons are:

- `keep`: digest supplied by repeated `--keep <digest>`;
- `pin-file`: digest supplied by newline-delimited `--pin-file <path>`;
- `shared-ref:<n>`: active Shared pmem marker count under
  `<image-store>/shared/<digest>/refs/`;
- `template:<fingerprint,...>`: committed snapshot-template manifests in the
  selected `--template-store` reference the digest;
- `min-age:<duration>`: the newest stored artifact for the digest is newer than
  `--min-age`.

`--min-age` accepts a whole-number duration with optional suffix `s`, `m`, `h`,
or `d`; a bare number is seconds. The command reports
`total_reclaimable_bytes` as the sum of candidate artifact sizes. `--execute`
is required to delete; omitted means dry-run.

Executable GC first acquires the image/template coordination lock in exclusive
mode, then scans and deletes candidates. Template build/commit takes the same
lock in shared mode while publishing manifests that reference image-store
digests. Deletion still calls `ImageStore::remove`, so a candidate cannot
bypass active Shared-marker checks, and the CLI rechecks committed template
references immediately before each removal.

## Output

Human output for `list`, `show`, `verify`, and `rm` uses one tabular shape:

```text
KIND    DIGEST    SIZE_BYTES    PATH
```

`--json` emits the standard CLI envelope:

```json
{
  "version": 1,
  "data": {
    "store": "/var/lib/m80-images",
    "images": [
      {
        "digest": "64 lowercase hex characters",
        "kind": "erofs",
        "size_bytes": 123,
        "path": "/var/lib/m80-images/..."
      }
    ]
  }
}
```

## Removal Guards

`m80 image rm <digest>` removes every artifact kind stored for that digest only
after both guards pass:

1. `ImageStore::remove` sees zero Shared active-use markers for the digest.
2. `TemplateStore::templates_referencing_image` finds no committed template
   manifest whose pmem input set includes the digest.

If a template references the image, the CLI renders a typed
`FcError::ImageStore(StoreError::ImageReferencedByTemplate { .. })` error and
exits non-zero. The template store does not own or delete image-store artifacts;
this guard only prevents accidental removal of a digest that existing templates
still need.

## Verification

`m80 image verify <digest>` re-hashes the stored bytes, validates metadata, and
opens artifact paths with `O_NOFOLLOW` through the image-store API. It is a
host-side store check, not a guest-kernel compatibility proof. Pmem attach and
guest DAX mount behavior remain covered by the real-KVM pmem tests.

## Tests

- `crates/m80-cli/tests/parse_args.rs` pins the argv shape.
- `crates/m80-cli/tests/help_smoke.rs` proves all image subcommands render
  help.
- `crates/m80-cli/tests/image_smoke.rs` covers build/import, list, show, verify,
  successful rm, JSON envelopes, human table output, template-reference rm
  refusal, and GC dry-run/execute retention without a KVM dependency.
- `crates/m80-image-store/tests/store.rs` covers list, describe, remove, and
  active Shared-ref refusal at the typed store layer, plus the shared/exclusive
  template-build versus GC coordination guard.
