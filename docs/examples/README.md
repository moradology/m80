# BootSpec Examples

These files are BootSpec YAML examples for the Phase E config surface. They are
parser fixtures as of `m80-q420k.5.3`; digest and fingerprint values
are synthetic but schema-valid lowercase sha256-shaped strings.

## Files

- `boot-spec-minimal.yaml` — no pmem layers and no snapshot template.
- `boot-spec-pervm-pmem.yaml` — one erofs image attached through
  `PmemSharing::PerVm`.
- `boot-spec-mixed-pmem.yaml` — one per-VM layer plus one shared layer guarded
  by an explicit same-operator trust acknowledgement.
- `boot-spec-snapshot-restore.yaml` — snapshot-template restore with typed
  post-restore hooks.
- `boot-spec-full.yaml` — mixed pmem plus snapshot-template restore.

## Commands

The Phase E CLI exposes these operator entry points for the examples:

```sh
m80 image build rust-toolchain --source ./toolchains/rust --out /var/lib/m80/images
m80 template build rust-warm --boot-spec docs/examples/boot-spec-full.yaml
m80 template show cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
```

`m80 template build` derives the real committed fingerprint from live typed
inputs. The fingerprint field inside snapshot-restore examples is the restore
selector used after a template exists.
