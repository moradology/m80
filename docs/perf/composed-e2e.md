# Composed E2E

> Status: scaffold and noisy-host diagnostic only. This document is not yet the
> verified-close receipt for `m80-q420k.6.5`; the close-quality rerun must
> replace the diagnostic values below with artifacts whose substrate records
> `allow_other_firecracker_vms=false` and an empty pre-existing Firecracker
> process list plus an empty post-run Firecracker process list and preflight
> artifact identity, and whose top-level `git_commit` records the measured
> source commit.

Beads: `m80-q420k.6.2`, `m80-q420k.6.3`, `m80-q420k.6.4`,
`m80-q420k.6.5`.

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`

## Method

Run date: 2026-05-17.

Command:

```sh
sudo -n env \
  PATH="$PATH" HOME="$HOME" CARGO_HOME="$HOME/.cargo" RUSTUP_HOME="$HOME/.rustup" \
  M80_COMPOSED_E2E_ALLOW_OTHER_VMS=0 \
  M80_COMPOSED_E2E_N=10 \
  M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB=32 \
  M80_COMPOSED_E2E_OUT_DIR=/tank/projects/m80/crates/m80-firecracker/benches/snapshots \
  M80_RUN_ROOT=/var/lib/m80-composed-e2e \
  M80_JAIL_UID="$(id -u)" \
  M80_JAIL_GID="$(id -g)" \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
  M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
  M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
  M80_ROOTFS_IMAGE=/tank/tmp/m80-build/post-restore-current/output.ext4 \
  M80_KERNEL_IMAGE=/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin \
  M80_KERNEL_KIND=stripped \
  cargo test --release -p m80-firecracker --test e2e_composed_real_kvm -- \
    --ignored composed_e2e_layered_warm_pool --nocapture
```

Host: Linux 6.17.0-23-generic x86_64, 48 logical CPUs,
`acpi-cpufreq` with `schedutil`, Firecracker v1.15.1, real `/dev/kvm`
rw, real jailer, noninteractive sudo. Rootfs image:
`/tank/tmp/m80-build/post-restore-current/output.ext4`
(`bfa35731760b9fbf06d41ffbfe500153d3247869dee75443dc836184b253613d`).
Kernel image:
`crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin`
(`143b2784a434cdf5de10920a59e2c875b66be63bacfa9ba8ddf93ac60f2bc6e3`).

Page cache was not dropped inside the run. The host was not fully quiet:
unrelated `t2-warm-slot-*` Firecracker processes were present, so this run is
not by itself a strict quiet-host verified close for `m80-q420k.6.3`.

The harness now refuses to run when pre-existing Firecracker processes are
present. `M80_COMPOSED_E2E_ALLOW_OTHER_VMS=1` permits a non-closeable
diagnostic run; each JSON artifact records the override and the pre-existing
process list under `substrate`. Close-quality JSON also records an empty
`substrate.post_run_firecracker_processes` list after teardown and a
`substrate.preflight_artifacts` object naming the resolved
Firecracker/jailer/kernel/rootfs inputs and manifest sha256s, plus runtime
substrate details for host kernel release, actual Firecracker version,
`/dev/kvm` stat output, and sudo uid. Each JSON artifact also records the full
measured `git_commit`.

Before closing `m80-q420k.6.2`, `.6.3`, or `.6.4`, after committing the
artifacts, run:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only composed-restore --only composed-memory --only composed-residue \
  --require-committed
```

The verifier rejects noisy-host override artifacts, missing fields, threshold
misses, teardown residue, and Shared payload layouts that do not match the
`.8.12` file-level DAX requirement. It also rejects artifacts that report
post-run Firecracker processes, image-store expected sets that are not exactly
the Shared and PerVm digests, or malformed template-store fingerprints. The
restore JSON must also carry warm-pool snapshots proving N=10 fill and N=10
concurrent lease shape, plus diagnostics for restore/load/probe/post-restore-hook
phases and workload exec completions for every lease. With `--require-committed`,
it also rejects
missing or malformed preflight artifact identity, artifacts absent from `HEAD`,
malformed `git_commit`, mismatched composed JSON identity/count/digest fields,
receipt text that does not mention the JSON artifacts' measured commit,
substrate identity, Shared image digest, and exactly one smoke artifact-write
line for each canonical composed JSON file, or artifacts with staged/unstaged
changes.

Before closing `.6.5` or the parent super-epic, run the verifier without
`--only` so it also checks the Phase C density, Phase D snapshot-template
artifact, and this receipt doc. For final parent close, include the close
reason and phase-parent checks:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --require-committed \
  --require-closed-beads \
  --require-parent-phases-closed
```

## Restore latency

Artifact: `composed-e2e-restore-N10.json`.

| N | fail count | P50 | P95 | P99 | bound |
|---:|---:|---:|---:|---:|---:|
| 10 | 0 | 116.400 ms | 126.456 ms | 126.456 ms | <= 200 ms |

The one-time template-build warmup took 10,237.670 ms and is recorded
separately. The N=10 restore samples exclude that build path.

## Host memory delta

Artifact: `composed-e2e-host-memory.json`.

Shared image digest:
`51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd`.

| field | bytes |
|---|---:|
| composed `after_n_attached_delta_bytes` | 58,433,536 |
| Shared erofs image | 33,558,528 |
| PerVm baseline delta | 805,371,904 |
| PerVm baseline copied Shared payload (`shared_image_bytes * N`) | 335,585,280 |
| derived `per_vm_overhead_bytes` | 46,978,663 |
| bound (`shared_image_bytes + per_vm_overhead_bytes * N`) | 503,345,158 |

The PerVm baseline uses the same large payload image mounted through
`PmemSharing::PerVm` for every lease, then subtracts `shared_image_bytes * N`
before deriving the non-shared per-VM overhead. The composed run stayed below
the bound, and the artifact records the Shared backing inode as
`dev=66308, ino=5636700`.

For the close-quality rerun, the Shared payload image must use the
`.8.12`-proven layout for file-level DAX: uncompressed erofs, non-inlined
`payload.bin`, and `dump.erofs --path=/payload.bin` reporting `Layout: 0` with
equal logical and on-disk size. The harness records that dump in the
host-memory artifact so the verifier can reject a run that accidentally
measured a compressed or inline erofs payload.

## Residue

Artifact: `composed-e2e-residue.json`.

| check | result |
|---|---|
| unexpected paths | `[]` |
| image store preserved | expected Shared digest `51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd` and PerVm digest `591aa08af783982da87fb19113fe2841068ccdfcd5dcfd2bc8a7d2ee0f580c9e` only |
| template store preserved | `13bf71c6468722d6a0a5bd2c57509c396c3abeb2b2a87e150838143b9e8899cb` |

Scanned roots:

- `/var/lib/m80-composed-e2e`
- `/tmp/m80-*`
- `/var/run/m80`
- `/var/lib/m80-images`
- `/var/lib/m80-composed-e2e/composed-e2e-templates-*/templates`

## Smoke evidence

```text
running 1 test
M80_COMPOSED_E2E_ARTIFACT /tank/projects/m80/crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json
M80_COMPOSED_E2E_ARTIFACT /tank/projects/m80/crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json
M80_COMPOSED_E2E_ARTIFACT /tank/projects/m80/crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json
M80_COMPOSED_E2E artifacts_dir=/tank/projects/m80/crates/m80-firecracker/benches/snapshots n=10 shared_digest=51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd per_vm_digest=591aa08af783982da87fb19113fe2841068ccdfcd5dcfd2bc8a7d2ee0f580c9e fingerprint=13bf71c6468722d6a0a5bd2c57509c396c3abeb2b2a87e150838143b9e8899cb
test composed_e2e_layered_warm_pool ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 24.94s
```
