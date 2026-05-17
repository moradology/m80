# Composed E2E

Beads: `m80-q420k.6.2`, `m80-q420k.6.3`, `m80-q420k.6.4`,
`m80-q420k.6.5`.

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`

## Method

Run date: 2026-05-17.

Measured git commit: `629c0c03ae0db24e1cab33f0c5f9e57df2de0880`.
Run ID: `composed-d5ee`.

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
rw, real jailer, noninteractive sudo.

Substrate identity:

- host kernel release: `6.17.0-23-generic`
- Firecracker version: `Firecracker v1.15.1

2026-05-17T11:44:58.005793166 [anonymous-instance:main] Firecracker exiting successfully. exit_code=0`
- /dev/kvm stat: `crw-rw---- root:kvm /dev/kvm`
- sudo uid: `0`
- firecracker_bin: `/opt/firecracker/bin/firecracker`
- firecracker_seccomp_filter: `/opt/firecracker/bin/firecracker-seccomp-filter.bin`
- jailer_bin: `/opt/firecracker/bin/jailer`
- jailer_harden_bin: `/opt/m80/bin/m80-jailer-harden`
- net_helper_bin: `/opt/m80/bin/m80-net-helper`
- expected_firecracker_version: `v1.15.1`
- rootfs image: `/tank/tmp/m80-build/post-restore-current/output.ext4`
- rootfs sha256: `bfa35731760b9fbf06d41ffbfe500153d3247869dee75443dc836184b253613d`
- kernel image:
  `/tank/projects/m80/crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin`
- kernel sha256: `143b2784a434cdf5de10920a59e2c875b66be63bacfa9ba8ddf93ac60f2bc6e3`

Page cache was not dropped inside the run. The artifact substrate records
`allow_other_firecracker_vms=false`,
`preexisting_firecracker_processes=[]`, and
`post_run_firecracker_processes=[]`.

Before closing `m80-q420k.6.2`, `.6.3`, or `.6.4`, after committing the
artifacts, run:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only composed-restore --only composed-memory --only composed-residue \
  --require-committed
```

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
| 10 | 0 | 118.847 ms | 129.401 ms | 129.401 ms | <= 200 ms |

The one-time template-build warmup took 10,468.516 ms and is recorded
separately. The N=10 restore samples exclude that build path.

The restore JSON also records the warm-pool fill/lease shape: after fill,
`ready=10`; during leases, `leased=10`, `ready=0`, `filling=0`; after discard,
`discarded=10`, `leased=0`. Per-lease diagnostics show restore/load/probe and
post-restore hook phases completed, with workload exec completions for every
lease.

## Host memory delta

Artifact: `composed-e2e-host-memory.json`.

Shared image digest:
`51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd`.

| field | bytes |
|---|---:|
| composed `baseline_before_fill_bytes` | 153,859,014,656 |
| composed `after_n_attached_bytes` | 153,841,803,264 |
| composed `after_n_attached_delta_bytes` | 17,211,392 |
| Shared erofs image | 33,558,528 |
| PerVm baseline delta | 453,300,224 |
| PerVm baseline copied Shared payload (`shared_image_bytes * N`) | 335,585,280 |
| derived `per_vm_overhead_bytes` | 45,330,023 |
| bound (`shared_image_bytes + per_vm_overhead_bytes * N`) | 486,858,758 |

The PerVm baseline uses the same large payload image mounted through
`PmemSharing::PerVm` for every lease. The bound uses observed same-run PerVm
baseline memory, `ceil(per_vm_baseline_delta / N)`, as the per-VM allowance.
The composed run stayed below the bound, and the artifact records the Shared
backing inode as `dev=66308, ino=5636700`.

The Shared payload image uses the `.8.12`-proven layout for file-level DAX:
uncompressed erofs, non-inlined `payload.bin`, and
`dump.erofs --path=/payload.bin` reporting `Layout: 0` with equal logical and
on-disk size. The harness records that dump in the host-memory artifact so the
verifier rejects a run that accidentally measured a compressed or inline erofs
payload.

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
- `/var/lib/m80-composed-e2e/composed-e2e-templates-m0t4XX/templates`

## Smoke evidence

```text
running 1 test
M80_COMPOSED_E2E_ARTIFACT /tank/projects/m80/crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json
M80_COMPOSED_E2E_ARTIFACT /tank/projects/m80/crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json
M80_COMPOSED_E2E_ARTIFACT /tank/projects/m80/crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json
M80_COMPOSED_E2E artifacts_dir=/tank/projects/m80/crates/m80-firecracker/benches/snapshots n=10 shared_digest=51453c4b9046488a66c1b5362466fc123f74bbf9a56284af4dc189e47775b2dd per_vm_digest=591aa08af783982da87fb19113fe2841068ccdfcd5dcfd2bc8a7d2ee0f580c9e fingerprint=13bf71c6468722d6a0a5bd2c57509c396c3abeb2b2a87e150838143b9e8899cb
test composed_e2e_layered_warm_pool ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 27.64s
```
