# Q420K Close Gates

This runbook is the operator checklist for verified close of `m80-q420k`.
It assumes the implementation scaffolds already compile and focuses only on
the remaining real-substrate measurement leaves:

- `m80-q420k.3.8`: Shared pmem density artifact.
- `m80-q420k.4.15`: snapshot-template restore latency artifact.
- `m80-q420k.6.2` through `.6.5`: composed e2e artifacts and receipt doc.

Do not use diagnostic override environment variables for close-quality runs:

- `M80_PMEM_SHARED_ALLOW_OTHER_VMS=1`
- `M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=1`
- `M80_COMPOSED_E2E_ALLOW_OTHER_VMS=1`

Those overrides are only for debugging a noisy host. A close-quality artifact
records `substrate_kind=real-kvm`, `preflight_required=true`,
`allow_other_firecracker_vms=false`, and an empty
`preexisting_firecracker_processes` list. It also records an empty
`post_run_firecracker_processes` list so the artifact proves no Firecracker
process leaked after teardown. The substrate records `preflight_artifacts`
with the resolved Firecracker, jailer, kernel, rootfs, and helper paths plus
the manifest kernel/rootfs sha256s, so the close evidence names the exact
inputs that were measured. Each artifact also records the measured
`git_commit`; it must be the full commit sha for the clean source tree used by
the run.

## Preflight

Use a quiet privileged runner with real KVM, sudo, the real Firecracker/jailer
binaries, and real rootfs/kernel artifacts. Before measuring, verify that no
unrelated Firecracker process is live:

```sh
./scripts/q420k-quiet-host-inventory.sh
```

The helper exits 0 only when no Firecracker process is present. If any process
appears, it prints owner inventory and exits 1. Stop there and ask the owner of
those VMs before draining or killing them. The measurement harnesses fail
closed on a noisy host.

For q420k close artifacts, the preflight artifact identity must report
`kernel_kind=stripped`. Do not pair a stock kernel path with
`M80_KERNEL_KIND=stripped`; the close verifier rejects stock-kernel
measurement artifacts for the Phase C, Phase D, and Phase F close gates.
Phase F composed artifacts also carry runtime substrate details; the verifier
requires host kernel >= 6.5, `/dev/kvm` rw stat output, sudo uid `0`, and
actual Firecracker version matching preflight.

The helper is equivalent to this manual inventory. To identify the owner
without changing host state, capture the process tree and cgroup first:

```sh
ps -eo pid,ppid,user,comm,args | rg 'firecracker|sandbox-executor-rs'
for p in $(pgrep firecracker); do
  printf '\nPID %s cgroup:\n' "$p"
  sed -n '1,5p' "/proc/$p/cgroup"
done
```

If a Firecracker process belongs to a Kubernetes pod cgroup, map the pod UID
back to its controller before requesting a drain:

```sh
kubectl get pod -A -o json \
  | jq -r '.items[]
    | select(.metadata.uid=="<pod-uid-from-cgroup>")
    | {namespace:.metadata.namespace,name:.metadata.name,node:.spec.nodeName,
       ownerReferences:.metadata.ownerReferences,
       containerStatuses:.status.containerStatuses} | @json'
```

If a Firecracker process is under a user systemd or tmux scope, capture the
scope status for the owner instead:

```sh
systemctl --user status '<scope-name>.scope' --no-pager
```

These commands are inventory only. Do not use them as approval to terminate the
processes.

If the inventory shows a noisy single-node host and no alternate real-KVM
runner is available, send an approval request that names the exact workloads to
pause, the measurement window, and the restore plan. The request should include:

- the Kubernetes controller, namespace, pod name, node, and Firecracker PIDs;
- any user systemd/tmux scope, owning process names, and Firecracker PIDs;
- the q420k leaves that will be measured while the host is quiet;
- confirmation that the pause is temporary and that Kubernetes workloads will
  be restored after the measurement run;
- a reminder that q420k agents must not drain, signal, kill, or scale these
  workloads without explicit operator approval.

Run close-quality measurements from a clean worktree except for the artifact
path being produced. The artifact guards reject measurements that record
uncommitted source changes.

## 1. Shared Pmem Density

Run from the repository root:

```sh
M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 \
M80_PMEM_SHARED_VM_COUNT=4 \
M80_PMEM_SHARED_CYCLES=10 \
M80_PMEM_SHARED_PAYLOAD_MIB=128 \
M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=131072 \
M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md \
M80_RUN_ROOT=/var/lib/m80-psd \
M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
M80_JAILER_BIN=/opt/firecracker/bin/jailer \
M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
M80_KERNEL_IMAGE=<real-stripped-kernel.bin> \
M80_KERNEL_KIND=stripped \
M80_ROOTFS_IMAGE=<real-rootfs.ext4> \
M80_FIRECRACKER_VERSION=v1.15.1 \
M80_JAIL_UID="$(id -u)" \
M80_JAIL_GID="$(id -g)" \
./scripts/smoke-pmem-shared.sh
```

Then verify the artifact:

```sh
python3 scripts/verify-q420k-artifacts.py --only pmem-density
```

The `pmem-density` selector verifies both
`docs/perf/pmem-shared-density.md` and `scripts/smoke-pmem-shared.sh`. The
script must be executable, retain the quiet-host fail-closed guard in
uncommented code, pass the exact helper/kernel/rootfs/run-root paths through
sudo, and emit the artifact's reproduction command. The selector also
parses this section's shell command block, so env values copied elsewhere in
the runbook do not satisfy the runnable close command. The artifact guard
requires the `## Trust model` text that the parent close/PR description must
paste or cite: `TrustDomainAck`, same trust domain, DAX cache-timing side
channel, and read-only Shared pmem jail bindings. It also requires host kernel
>= 6.5, `/dev/kvm` rw stat output, sudo/root uid evidence, and Firecracker
version output matching preflight. The measured Shared image
digest must be a lowercase sha256 and its image path must be absolute and must
contain that digest. The reproduction command must also match the Firecracker,
jailer, helper, kernel, rootfs, version, and kernel-kind values in the
artifact's `preflight_artifacts` block. Those command checks parse exact env
assignments; prefix values such as `VM_COUNT=40` do not satisfy an expected
`VM_COUNT=4`. The `Samples` table must contain exactly one row per cycle, each
row's delta must recompute from the
MemAvailable before/after values, and the summary max delta must equal the
table max. The artifact parser reads the reproduction command from
`## Reproduction`, host substrate fields from `## Substrate`, numeric bounds
and image identity from `## Observable`, and teardown marker fields from
`## Teardown`; copies outside those sections do not satisfy the close artifact.

After committing the artifact, rerun the close guard with git-state checks:

```sh
python3 scripts/verify-q420k-artifacts.py --only pmem-density --require-committed
```

That committed-state check covers both the Markdown artifact and the smoke
script.

Close reason:

```text
verified: docs/perf/pmem-shared-density.md @ <commit-sha>
```

## 2. Snapshot-Template Restore Latency

Run the N=20 x 3 snapshot-template bench described in
`docs/perf/snapshot-template-restore.md`, writing:

```text
crates/m80-firecracker/benches/snapshot_template_restore_latency.json
```

Replace that doc's pending `## Smoke evidence` section with exactly one bench
stderr line from the same quiet-host run, including
`snapshot-template restore: load=idle runs=3 n=20`, the recorded `p99=...`,
and `output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json`.
The JSON and doc must also pin `M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0`,
`target_ready=1`, `M80_SNAPSHOT_BENCH_VCPU_COUNT`,
`M80_SNAPSHOT_BENCH_MEM_SIZE_MIB`, and `M80_RUN_ROOT` rather than relying on
defaults. The JSON must record exactly `n_per_run=20`, `runs=3`, and
`samples_total=60`. It must include `samples_us` and `sample_details` arrays
whose lengths match `samples_total`, a `runs_detail` entry for each run, and
exactly one `sample_details` row for every `(run, cycle)` in the N x runs
matrix. The recorded `p99` must recompute from those samples. Remove pending or
diagnostic-only text from the receipt doc before close; the verifier rejects
stale diagnostic markers and cross-checks the doc against the JSON artifact's
measured git commit and preflight identity in `## Artifact identity`,
command-block env assignments, and smoke-section p99. Prose outside those
sections does not satisfy those requirements.

Then verify the artifact:

```sh
python3 scripts/verify-q420k-artifacts.py --only snapshot-template
```

After committing the artifact, rerun:

```sh
python3 scripts/verify-q420k-artifacts.py --only snapshot-template --require-committed
```

The `snapshot-template` selector includes
`docs/perf/snapshot-template-restore.md`, so this command verifies both the
JSON artifact and the committed reproduction doc.

Close reason:

```text
verified: crates/m80-firecracker/benches/snapshot_template_restore_latency.json @ <commit-sha>
```

## 3. Composed E2E

Run the composed e2e command from `docs/perf/composed-e2e.md` on the quiet
host. It must write all three artifacts:

```text
crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json
crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json
crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json
```

Replace the diagnostic values in `docs/perf/composed-e2e.md` with the
close-quality host context, tables, and smoke paste from that run. Then verify
the composed subset. This also checks that the three JSON artifacts agree on
`git_commit`, `run_id`, substrate, target count, and lowercase sha256 Shared
image digest, and that the Phase F cardinality is exactly N=10 across restore,
host-memory, and residue. The shared `run_id` is the close guard for the
"same invocation emits all three artifacts" requirement. The restore artifact's
`samples_ms` array must back
its `count`, `target_ready` must equal `count`, and P50/P95/P99 values must
recompute from that same sample array. Runtime substrate fields must show host
kernel >= 6.5, `/dev/kvm` rw, sudo uid `0`, and actual Firecracker version
matching preflight. The restore JSON must also prove all 10 warm-pool slots
filled, all 10 were leased concurrently, each lease recorded restore/load/probe
and post-restore-hook diagnostics, and workload exec completions were observed.
The residue artifact must also record lowercase sha256 Shared and PerVm
image-store digests, make the expected image set exactly those two digests, and
use lowercase sha256 template-store fingerprints. The full guard
also checks that the receipt doc's `## Method` section mentions the measured
git commit, run ID, and substrate identity from those JSON artifacts, that the
receipt mentions the Shared image digest, and that it has green
`composed_e2e_layered_warm_pool` test-result lines, with exactly one
`M80_COMPOSED_E2E_ARTIFACT` smoke line inside `## Smoke evidence` for each
canonical composed JSON artifact. It also cross-checks the receipt's
command-block env assignments, restore-section P50/P95/P99, host-memory-section
byte values, residue-section scan roots, and the Shared image digest in both the
host-memory and residue sections against the JSON artifacts; prose outside the
command block does not satisfy command requirements. The
host-memory subset check also recomputes `bound_bytes` from
`shared_image_bytes`,
`per_vm_overhead_bytes`, and `n_attached`, verifies the attached-memory delta
from the raw MemAvailable checkpoints, and requires the Shared image path to
contain the Shared digest. It also checks that the same-run PerVm baseline
section agrees with the top-level Shared digest, Shared bytes, attached count,
payload-copy bytes, attached-memory delta, derived per-VM overhead, and
all-slots-leased snapshot. The residue subset requires an
explicit scanned run-root entry alongside `/tmp/m80-*`, `/var/run/m80`, the
image store, and the template store; its `leased_run_dirs` entries must be
absolute path strings under the scanned run root:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only composed-restore --only composed-memory --only composed-residue
```

After committing the artifacts, rerun the JSON subset with git-state checks:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only composed-restore --only composed-memory --only composed-residue \
  --require-committed
```

Before closing `.6.5`, after replacing and committing the receipt doc, run the
full guard so `docs/perf/composed-e2e.md` is checked too:

```sh
python3 scripts/verify-q420k-artifacts.py --require-committed
```

For a narrower receipt-doc preflight, `--only composed-doc` checks
`docs/perf/composed-e2e.md` plus the three composed JSON artifacts it
interprets.

Close reasons:

```text
verified: crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json @ <commit-sha>
verified: crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json @ <commit-sha>
verified: crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json @ <commit-sha>
verified: docs/perf/composed-e2e.md @ <commit-sha>
```

The `.6.5` receipt close reason must include the three JSON measurement
`verified:` lines above as references, in addition to its own
`docs/perf/composed-e2e.md` verified line. The final verifier enforces those
references and requires their commits to match the already-closed `.6.2`,
`.6.3`, and `.6.4` close reasons when `--require-closed-beads` is present.

## 4. Parent Close Check

After all five measurement artifacts and the composed receipt doc are
committed, close the measurement leaves with the close reasons above, then
close their phase parents:

- `m80-q420k.3` with the same
  `verified: docs/perf/pmem-shared-density.md @ <commit-sha>` line used by
  `.3.8`.
- `m80-q420k.4` with the same
  `verified: crates/m80-firecracker/benches/snapshot_template_restore_latency.json @ <commit-sha>`
  line used by `.4.15`.
- `m80-q420k.6` with the same
  `verified: docs/perf/composed-e2e.md @ <commit-sha>` line used by `.6.5`.

Then run the full guard:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --require-committed \
  --require-closed-beads \
  --require-parent-phases-closed
br blocked --json | jq '[.[] | select(.id == "m80-q420k" or .id == "m80-q420k.3" or .id == "m80-q420k.4" or .id == "m80-q420k.6")]'
br dep cycles --json
```

Do not add `--only` to the parent or super-epic close commands. Subset checks
are only for leaf preflight/debugging, and the verifier rejects `--only` when
combined with `--require-parent-phases-closed` or
`--require-super-epic-closed`. It also rejects parent-close mode without both
`--require-committed` and `--require-closed-beads`, and rejects super-epic
close mode without parent-close mode.

`m80-q420k` is ready to close only when the verifier passes and the filtered
`br blocked` command shows no remaining blockers for `m80-q420k`, `.3`, `.4`,
or `.6`. The verifier also checks those q420k blocker rows and dependency
cycles when `--require-parent-phases-closed` is present; the explicit `br`
commands above are retained as operator-readable evidence. The verifier checks every measurement leaf's
`verified: <artifact> @ <commit-sha>` close reason, verifies that each cited
commit exists in `HEAD` history and contains the named artifact, verifies that
the cited commit descends from the artifact's measured `git_commit`, verifies
that the artifact has not changed since that cited commit, verifies the Phase C,
Phase D, and Phase F parent close reasons reuse the matching child evidence
commits, and confirms Phase 0 plus Phases A-F are closed.

After closing `m80-q420k`, rerun the same guard with the post-close check:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --require-committed \
  --require-closed-beads \
  --require-parent-phases-closed \
  --require-super-epic-closed
```

The super-epic close reason must reuse the same
`verified: docs/perf/composed-e2e.md @ <commit-sha>` evidence commit as
`m80-q420k.6`.

`m80-q420k.8` is a rolling Phase G holding area and is not a super-epic close
blocker; do not use its open measurement followups (`.8.9`, `.8.16`) as a
reason to hold the A-F parent close.

## 5. Phase G Followups

Phase G leaves are handled after, or in parallel with, the A-F close gate. They
use the same measurement discipline, but they are not inputs to the
`m80-q420k` super-epic close decision.

`m80-q420k.8.16` is the ext4 overlay-template fallback recheck. The current
artifact path is:

```text
docs/perf/ext4-overlay-template-clone.md
```

Before closing `.8.16`, run:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only ext4-overlay \
  --require-committed
```

The leaf is `requires-verified-close`; close it only with explicit operator
approval and a close reason in this form:

```text
verified: docs/perf/ext4-overlay-template-clone.md @ <commit-sha>
```

`m80-q420k.8.9` is the DAX memory-pressure side-channel investigation. It
must produce:

```text
docs/perf/pmem-dax-memory-pressure.md
```

Use the command block in `docs/perf/measurement-playbook.md` under "Q420K DAX
Memory Pressure". Do not use `M80_PMEM_DAX_MEMORY_PRESSURE_ALLOW_OTHER_VMS=1`
for verified evidence. If the quiet-host inventory helper reports unrelated
Firecracker processes, stop and collect owner approval or move to a quiet real
KVM host. The receipt must record the full measured commit, quiet-host
Firecracker substrate JSON with empty pre-run and post-run Firecracker process
lists, preflight artifact identity for the measured
Firecracker/jailer/helper/kernel/rootfs inputs, a reproduction command whose
env assignments match that preflight identity, lowercase sha256 Shared image
digest with an absolute image path containing that digest, ordered
baseline/post-pressure latency percentiles, and the cross-guest signal delta
recomputed from P50 values. After committing the artifact, run:

```sh
python3 scripts/verify-q420k-artifacts.py \
  --only dax-memory-pressure \
  --require-committed
```

The `.8.9` close reason is:

```text
verified: docs/perf/pmem-dax-memory-pressure.md @ <commit-sha>
```
