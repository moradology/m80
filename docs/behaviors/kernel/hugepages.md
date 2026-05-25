# Firecracker 2 MiB Hugepage Backing

Bead: `m80-2ggw.5.3`

Firecracker v1.15.1 exposes guest-memory hugepage backing through
`MachineConfig.huge_pages = "2M"`. This is hugetlbfs-backed memory, not a
transparent-hugepage toggle. The host must reserve enough free 2 MiB hugepages
before launch; for m80's default 512 MiB VM, that means at least 256 free
hugepages.

m80 exposes this as an explicit opt-in:

- `m80_firecracker_client::HugePageConfig::Hugetlbfs2M`
- `m80_firecracker::SandboxConfig::huge_pages_2m`
- `m80 run --huge-pages-2m`

The default is false. Admission rejects `huge_pages_2m = true` with an odd
`mem_size_mib`, because Firecracker validates hugetlbfs memory size at 2 MiB
granularity.

## Measurement

Host state before the run:

- `HugePages_Total: 0`
- temporary setting for the opt-in run:
  `/sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages = 300`
- restored after the run: `HugePages_Total: 0`

Command shape:

```sh
N=30 WARMUP=2 KIND=minimal SKIP_LOADED=1 \
  BENCH_ARTIFACT_DIR=/tank/tmp/m80-2ggw-hugepages-baseline-n30 \
  IMAGE_BUILD_DIR_MINIMAL=/opt/m80/versions/v0.2.20/artifacts \
  M80_RUN_ROOT=/tank/tmp/m80r \
  M80_BIN=/tank/tmp/m80-bench-wrapper \
  ./scripts/bench-cold-launch.sh

sudo sh -c 'echo 300 > /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages'

N=30 WARMUP=2 KIND=minimal SKIP_LOADED=1 \
  BENCH_ARTIFACT_DIR=/tank/tmp/m80-2ggw-hugepages-2m-n30 \
  IMAGE_BUILD_DIR_MINIMAL=/opt/m80/versions/v0.2.20/artifacts \
  M80_RUN_ROOT=/tank/tmp/m80r \
  M80_BIN=/tank/tmp/m80-bench-wrapper-huge \
  ./scripts/bench-cold-launch.sh
```

The wrappers only set `M80_SKIP_CHECK_NESTED_VIRT=1`,
`M80_ARTIFACT_DIR=/opt/m80/versions/v0.2.20/artifacts`, and, for the second
run, insert `--huge-pages-2m` after `m80 run`.

Artifacts:

| cell | path | sha256 |
|---|---|---|
| baseline wallclock | `/tank/tmp/m80-2ggw-hugepages-baseline-n30/cold-launch.csv` | `fa8ff390bbbdf7fc1655108712e234cd576e7627510a0e7b75eee2131cd78b6a` |
| baseline phases | `/tank/tmp/m80-2ggw-hugepages-baseline-n30/cold-launch-phases.csv` | `1e5d5201f18ecd7704f42f3c63ee6659b1692c2e70625b1c24753b2ed97950b4` |
| baseline snapshot | `/tank/tmp/m80-2ggw-hugepages-baseline-n30/snapshots/2026-05-25T01:55:32+00:00.json` | `0fc100757b8a43643e9c412da8f17a4114ebfa1d92926c1fe9bf8eabfa9fd58d` |
| hugepages wallclock | `/tank/tmp/m80-2ggw-hugepages-2m-n30/cold-launch.csv` | `94daab0ceaf0b76c5f9160f593b90226e8e0791ae1b0dedb5eb4e0c42e9f4e43` |
| hugepages phases | `/tank/tmp/m80-2ggw-hugepages-2m-n30/cold-launch-phases.csv` | `c4c179a57142f200b09ba13ba9a0a3b17657100c3f5ba8cce2f0ca92cff826a0` |
| hugepages snapshot | `/tank/tmp/m80-2ggw-hugepages-2m-n30/snapshots/2026-05-25T01:57:02+00:00.json` | `c8631d36a89abf1ed8be86532749aaa97ef860d47e282424e8bd64b22568e2a2` |

Result:

| metric | baseline | hugepages 2M | delta |
|---|---:|---:|---:|
| wallclock P50 | 1630 ms | 1631 ms | +1 ms |
| `phase_12b_ready_accept` P50 | 943.470 ms | 926.905 ms | -16.565 ms |
| `phase_12a_instance_start` P50 | 21.038 ms | 8.715 ms | -12.323 ms |

## Decision

Keep hugepages as an explicit opt-in and do not change the default. The
default-switch threshold for this bead was a >=50 ms improvement on the
standard 512 MiB minimal/idle launch. This host measured a 16.565 ms
`phase_12b_ready_accept` improvement and no wallclock P50 improvement.

Do not add guest `CONFIG_TRANSPARENT_HUGEPAGE` symbols for this behavior.
Firecracker's `huge_pages` field uses host hugetlbfs pages for guest memory;
guest-side THP is a separate kernel behavior and does not enable this API path.
