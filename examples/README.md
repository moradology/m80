# m80 Examples

These examples show m80 as a constrained-process wrapper. Every command assumes
the selected runtime profile already contains the requested program.

- [echo-hello](echo-hello/) — smallest pipe-mode run.
- [workspace-roundtrip](workspace-roundtrip/) — expose a host directory and
  write changes back on success.
- [network-egress](network-egress/) — run with outbound egress enabled.

Run scripts with `sh <example>/run.sh`; they are intentionally tiny so the
exact `m80 run` invocation stays visible.

## BootSpec Config

Draft BootSpec YAML examples for pmem layers and snapshot-template restore live
under [`docs/examples/`](../docs/examples/). Those files are parser targets for
Phase E and use placeholder digests until the config-schema bead lands.
