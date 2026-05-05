# Perf roadmap, extended

**Date:** 2026-05-04
**Companion:** `docs/planning/agent-roadmap.md` (the four-step path), `docs/planning/storage-pivot-bead-plan.md`, `docs/planning/wire-features-bead-plan.md`.
**Scope:** the four cold-launch latency epics — `m80-f2zc` (storage pivot), `m80-ci9i` (stripped kernel), `m80-rrp.3` (snapshot/restore), `m80-qokt.2` (persistent VM). The two wire-protocol epics (`m80-6zim`, `m80-5vha`) are explicitly out of scope for this pass and are tracked in their own planning doc.

This document extends each epic with: explicit risk registers, post-IMPL smoke checkpoints, rollback notes, cross-epic dependencies, effort *ranges*, and confidence-intervaled savings. It is the auditable bridge from "current 1.9 s minimal cold launch" to "sub-200 ms warm-pool launch". Each section ends with a copy-pasteable block of `br create` / `br update` commands.

---

## Executive summary

- **The chain stays four steps**, but each step now has an explicit risk register and a post-IMPL smoke checkpoint that gates BENCH; we have lost time before to hypothesis-first debugging (see CLAUDE.md "diagnostics before hypotheses"), and tighter checkpoints catch structural failures before they hide in N=30 noise.
- **Storage pivot (`m80-f2zc`) and stripped kernel (`m80-ci9i`) are the only "high-confidence" steps.** Both have well-trodden mechanisms and existing point measurements (Firecracker test framework's RO-base sharing for the former; community kernel-strip recipes for the latter). Expected savings are stated as ranges; lower-bound savings still get us to ≈400-600 ms cold.
- **Snapshot/restore (`m80-rrp.3`) is the headline unlock and the highest-risk leaf.** No production runtime we surveyed (smolvm, kata-containers) has wired Firecracker's snapshot REST surface. m80 is unclaimed territory; expected save is medium-confidence, vsock survival across snapshot is the live unknown, and we add a discrete "vsock-survival research" sub-leaf before committing IMPL.
- **Persistent VM (`m80-qokt.2`) is decomposed into a 6-leaf chain** (DESIGN, API mutation, sequential-exec test, cancellation contract, idle-timeout, BENCH+DOCS). It is orthogonal to the latency chain and can land any time after `m80-qokt.1` (the in-tree adapter) — no perf-chain dependency.
- **The cross-epic dependency that matters is `m80-f2zc.5` ↔ `m80-ci9i`**: the stripped kernel must include `CONFIG_OVERLAY_FS=y` or storage pivot regresses to non-functional. We fold that constraint into the stripped-kernel DESIGN leaf so it cannot be missed.

---

## How to read each epic section

For each of the four epics:

1. **Risk register** — concrete failure modes with mitigations. Where the mitigation is "another bead does X", that bead is named.
2. **Existing leaves, extended** — additional acceptance criteria, rollback notes, and confidence-intervaled savings appended to leaves that already exist.
3. **New leaves to file** — for missing decompositions or for the new "smoke checkpoint" / "risk register" leaf where applicable.
4. **`br create` / `br update` blocks** at the end — ready to copy-paste.

Effort sizing: **S** = 2-4 h, **M** = 1 day (upper end 1.5 d), **L** = 2-3 days (upper end 4 d), **XL** = a week+ (upper end 2 w). The upper bound is the realistic estimate, not the optimistic one. If a leaf is honestly XL we file it as XL — agent-grade infra is not improved by lying to ourselves about effort.

Savings confidence:
- **High** = mechanism well-documented in firecracker community, point measurement available, our setup matches.
- **Medium** = mechanism is documented, the published numbers come from a setup similar to ours but not identical.
- **Low** = no precedent or our setup diverges materially; the bead's BENCH leaf is the authoritative source.
- **Unknown** = no precedent at all; will be measured.

---

## 1. Storage pivot — `m80-f2zc`

**Headline:** delete the 727 ms `Rootfs::clone` full-file copy; replace with shared RO base + per-VM sparse ext4 overlay + in-guest overlayfs + `pivot_root`. Saves a measured 727 ms ± a noise floor of ~30 ms; that piece is **high-confidence** (we have point measurement; mechanism is what runc, crun, kata, and firecracker-containerd all do).

### 1.1 Risk register

| # | Failure mode | Pre-condition | Detection | Mitigation |
|---|---|---|---|---|
| R1 | firecracker-ci 5.10.245 kernel ships **without** `CONFIG_OVERLAY_FS=y` (built-in) | image-build picks up stock kernel | `m80-f2zc.5` build-time grep of `IKCFG`; runtime `cat /proc/filesystems \| grep overlay` returns empty | `m80-f2zc.5` escalates from "verify" to "build custom kernel" — add `CONFIG_OVERLAY_FS=y` + `CONFIG_OVERLAY_FS_XINO_AUTO=y`. Cost: M (1 day). Folded preemptively into `m80-ci9i` (stripped kernel ships overlayfs by construction). |
| R2 | overlay mount succeeds but `pivot_root(".",".")` fails as PID 1 | kernel newer than 5.4 with restricted `pivot_root` | guest panics; host sees `phase_12b_ready_accept` time out at 60 s; run-dir preserved | Per design: panic-fast, no retry. `m80-f2zc.4` integration test must boot end-to-end on the same kernel CI uses; the test is the gate. |
| R3 | kata `pivot_rootfs` lift has subtle bug (e.g. our scopeguard `defer!` ordering differs) | `m80-f2zc.4` lifts the function but doesn't reproduce all kata test fixtures | `pid_one_pivot` test asserts FD-open + chdir but stubs the syscall; real failure only surfaces in integration | Add a smoke checkpoint (new leaf `m80-f2zc.5b`) — single end-to-end launch with `M80_PHASE_TRACE=1`, asserts probe-after-pivot byte lands on overlay disk. |
| R4 | RO base file's page cache is *not* shared across VMs (e.g. host filesystem opens fresh inode) | run-dir filesystem is FUSE/9p/overlayfs-on-host | observable as repeat 256 MiB working-set populate per VM; bench's 16-VM concurrent number regresses | `m80-preflight` rejects non-native run-root filesystems (already on the followup-deferred list per storage-pivot-bead-plan §3 question 6). Bench `m80-f2zc.7` reads `/proc/meminfo` Cached delta over 16 launches; if delta > 256 MiB × 16 ÷ 4 the assumption is wrong and we file a regression. |
| R5 | sparse-overlay `mkfs.ext4 -F` is slow on overlay path (>50 ms) on some host fs | older kernel or quirky filesystem | `phase_3_storage_prep` per-call in the new bench shows >50 ms | Acceptable up to ~80 ms (we still come out ahead by ~650 ms). >100 ms triggers a follow-up: pre-cache a freshly-mkfs'd 512 MiB ext4 template and `cp --reflink=auto` on demand (out of scope for this epic; file as `m80-f2zc.{tbd}` if hit). |
| R6 | overlay disk fills during long-running VM and writes start failing | exec writes more than `overlay_size_bytes` (default 512 MiB) | `m80-guestd` sees ENOSPC | Surfaces as a normal exec error; not a launch issue. Document in `m80-f2zc.8` README: "VMs are short-lived; overlay sized for the working set, not for accumulation." Persistent-VM mode (`m80-qokt.2`) re-evaluates this. |
| R7 | base file is mutated post-launch by some out-of-band path (`m80-image-build` re-run mid-flight, race) | concurrent build + launch | base sha256 mismatch surfaces in next launch | `m80-image-manifest` already verifies before mount. **Smoke checkpoint** (`m80-f2zc.5b`) explicitly hashes base-file pre/post a launch and asserts equality. |

Mitigations R1, R3, R7 are now leaf-tracked (R1 cross-epic to `m80-ci9i.1`; R3 and R7 in the new smoke leaf `m80-f2zc.5b`).

### 1.2 Existing leaves, extended

The eight leaves under `m80-f2zc` are well-described already (see `docs/planning/storage-pivot-bead-plan.md`). Extensions only:

- **`m80-f2zc.1` (DESIGN).** Append a "Risk register" section to the design doc itself, cross-referencing R1-R7 above. Extra acceptance: design doc lists which BENCH numbers from R4 are measured by `m80-f2zc.7`. Effort unchanged: **S (3-4 h)**.
- **`m80-f2zc.2` (m80-storage IMPL).** Add rollback note: `git revert <commit>` is sufficient — no on-disk state created by this leaf. Existing run-dirs predating the revert are not affected. Effort unchanged: **M (1 d)**. Confidence on the wire-up itself: high; this is a refactor.
- **`m80-f2zc.3` (m80-firecracker IMPL).** Rollback note: revert is sufficient; jailer chroot bind-mount additions are torn down per launch. **Confirm the BENCH leaf rebuilds from a clean run-root** (the run-root layout schema doesn't bump; if it did, document migration). Effort unchanged: **S (4 h)**.
- **`m80-f2zc.4` (m80-guestd IMPL).** Rollback note: revert is sufficient at the host level; *any guest images already produced and cached* still have the new PID-1 baked in. To fully roll back, both this commit AND `m80-image-build`'s next image production must be reverted, OR a new image artifact built. Capture this in the leaf description. Effort unchanged: **L (2-3 d, upper 4 d)** — kata lift is fast, integration shake-out is the time sink. R3 mitigation: `m80-f2zc.5b` smoke checkpoint blocks BENCH.
- **`m80-f2zc.5` (kernel CONFIG_OVERLAY_FS).** Add explicit cross-epic note: when `m80-ci9i` (stripped kernel) lands, `m80-ci9i.1` DESIGN must include `CONFIG_OVERLAY_FS=y` AND `CONFIG_OVERLAY_FS_XINO_AUTO=y` in the keep-list (they are not on the current keep-list; this is a real gap). Track as `m80-f2zc.5 → m80-ci9i.1` related-dep. Effort unchanged: **S-M (2 h - 1 d)**.
- **`m80-f2zc.6` (TESTS).** Confidence on tests is high; mechanical. Effort unchanged: **M (1 d)**.
- **`m80-f2zc.7` (BENCH).** Add to acceptance: report 16-VM concurrent launch with **`/proc/meminfo` Cached delta** (R4 mitigation). Expected save: **700-770 ms (high confidence)**. Lower bound preserved by the 727 ms point measurement; upper bound unlikely to exceed ~770 ms unless ready-probe also accelerates due to lower memory pressure. Pre-condition: R1-R4 not triggered. Effort: **S (3 h)** — pipeline exists.
- **`m80-f2zc.8` (DOCS).** Effort unchanged: **S (1-2 h)**.

### 1.3 New leaf: `m80-f2zc.5b` smoke checkpoint

Between IMPL (`.4`) and BENCH (`.7`). One end-to-end launch with explicit invariants:

1. SHA256 of the base file unchanged before/after the launch (R7).
2. Overlay file size grew by < 100 KB during a `/bin/echo` exec (sanity bound).
3. `mount` output inside the guest, captured before exec, shows overlayfs at `/` and base at `/lower` is gone post-pivot (i.e. the old mount tree was actually detached).
4. `phase_12b_ready_accept` in the M80_PHASE_TRACE output completes inside the new shorter budget (≤ 250 ms is a reasonable upper bound post-pivot; if it's >300 ms, something is wrong — likely vdb/vdc PUT order or overlay mkdir cost — flag and don't proceed to BENCH).

**Effort:** **S (1-2 h)**. Run, record, gate.
**Rollback:** N/A — purely a verification step.
**Acceptance:** smoke notes appended to bead, plus a one-paragraph entry in `docs/perf/cold-launch.md` "smoke checkpoint" section.

### 1.4 New leaf: `m80-f2zc.0` risk register (or fold into `.1`)

Folded into `m80-f2zc.1` DESIGN as a "Risk register" section per §1.2 above. **Not a separate leaf.** Reasoning: the risk register is part of the design contract; separating it creates a coordination problem (what's authoritative?). One leaf, one doc.

### 1.5 Cross-epic dependencies (storage pivot)

- `m80-f2zc.5` **gates** `m80-ci9i.1` design — stripped kernel must include `CONFIG_OVERLAY_FS=y`.
- `m80-f2zc.{2,3,4}` landed **enables** `m80-rrp.3.{2,4}` to assume drive layout vda=base RO + vdb=overlay RW + vdc=workspace RW. Without storage pivot, snapshot/restore would have to capture the per-VM cloned 256 MiB rootfs every time, which makes "snapshot diff" pointless. Track as `m80-rrp.3.4 → m80-f2zc.4` related-dep.

### 1.6 `br` commands

```bash
# 1. Append risk-register summary to f2zc.1 DESIGN.
br update m80-f2zc.1 --notes "extend: append risk register R1-R7 to docs/design/storage-overlay.md; cross-link to m80-ci9i.1 for CONFIG_OVERLAY_FS=y in stripped kernel keep-list. See docs/planning/perf-roadmap-extended.md §1.1."

# 2. Add rollback note to each IMPL leaf.
br update m80-f2zc.2 --notes "rollback: git revert is sufficient; no on-disk state created by this leaf."
br update m80-f2zc.3 --notes "rollback: git revert is sufficient; jailer chroot bind-mounts torn down per launch. Confirm BENCH rebuilds from clean run-root."
br update m80-f2zc.4 --notes "rollback: git revert is necessary AND any cached image artifacts must be rebuilt (PID-1 path baked into image). See docs/planning/perf-roadmap-extended.md §1.2."

# 3. Cross-epic dep: m80-f2zc.5 -> m80-ci9i.1 (related, not parent).
br dep add m80-ci9i.1 m80-f2zc.5 --type related

# 4. New smoke-checkpoint leaf between IMPL and BENCH.
br create "SMOKE: post-IMPL storage-pivot end-to-end probe (base sha256, overlay growth, mount tree, ready_accept budget)" \
  --type task --priority P2 --parent m80-f2zc \
  --labels active,storage,smoke,v02 \
  --deps "parent-child:m80-f2zc.4,parent-child:m80-f2zc.5" \
  -d "Single end-to-end launch with M80_PHASE_TRACE=1, gates BENCH (m80-f2zc.7). Asserts: (1) SHA256 of base file unchanged pre/post launch; (2) overlay file grew <100 KB during /bin/echo exec; (3) inside-guest 'mount' output shows overlayfs at /, /lower torn down post-pivot; (4) phase_12b_ready_accept <=250 ms (>300 ms blocks BENCH and triggers diagnostics-first triage per CLAUDE.md). Output: smoke notes appended to bead + paragraph in docs/perf/cold-launch.md 'smoke checkpoint' section. Effort: S (1-2 h). Rollback: N/A — verification step. See perf-roadmap-extended.md §1.3."

# 5. Confidence-and-savings note on the BENCH leaf.
br update m80-f2zc.7 --notes "expected save: 700-770 ms (high confidence; 727 ms point-measured pre-pivot). Acceptance extension: report 16-VM concurrent launch with /proc/meminfo Cached delta (R4 mitigation)."

# 6. Cross-epic dep: m80-rrp.3.4 wants storage pivot landed.
br dep add m80-rrp.3.4 m80-f2zc.4 --type related
```

---

## 2. Stripped kernel — `m80-ci9i`

**Headline:** purpose-built kernel with the smallest config m80 needs (vsock, virtio-blk/net/mmio, ext4, **overlayfs**, devtmpfs, 8250 console; everything else off) plus a trimmed cmdline. Expected save **500-700 ms (high confidence)** on `phase_12b_ready_accept`. Lower bound: 500 ms (firecracker community-reported "150-300 ms userspace" with stripped kernels on idle hosts; we'd be in this range). Upper bound: 700 ms (full delta if our current 890 ms is mostly bloat). Mechanism well-documented; we just have to do it.

### 2.1 Risk register

| # | Failure mode | Detection | Mitigation |
|---|---|---|---|
| R1 | Stripped kernel **lacks `CONFIG_OVERLAY_FS=y`** → boot fails on `m80-f2zc.4`'s overlay mount | `phase_12b_ready_accept` times out; guest panics with overlay ENODEV | DESIGN leaf `m80-ci9i.1` keep-list MUST include `CONFIG_OVERLAY_FS=y` + `CONFIG_OVERLAY_FS_XINO_AUTO=y`. Cross-epic dep `m80-f2zc.5 → m80-ci9i.1` is the gate. |
| R2 | Stripped kernel breaks on a host CPU with feature we accidentally disabled (e.g. AES-NI turned off when guestd uses crypto) | guestd panics on first crypto op | Build matrix smoke: boot + run `/bin/echo` and `/bin/sha256sum` of a known fixture. If sha256sum fails, kernel config dropped a needed feature. New smoke leaf (`m80-ci9i.3b`) covers this. |
| R3 | Boot delta is **less than expected** (e.g. saves only 200 ms instead of 500-700 ms) | BENCH leaf shows < 500 ms save | Honest. We document the actual number; the chain still works (cumulative path to <200 ms warm holds even with conservative cold-boot save). No mitigation; accept the data. |
| R4 | Stripped kernel boots fine in CI but flakes on a different host CPU family (firecracker prod usually AWS c5.metal; we test on a 48-core ryzen) | Out-of-tree user reports flake | Document the test platform in `m80-ci9i.1` DESIGN; commit to "boot-tested on x86_64 with KVM"; defer multi-arch to a later bead per existing non-goal. |
| R5 | Build environment (Docker container) drifts; vmlinux sha changes from one build to the next without code changes | manifest sha mismatch surprises a developer | `m80-ci9i.2` build is **deterministic**: pinned base image (Ubuntu 22.04 by digest, not tag), `KBUILD_BUILD_TIMESTAMP=0`, `SOURCE_DATE_EPOCH=...`. Acceptance extension: same input twice ⇒ byte-identical vmlinux. |
| R6 | Custom kernel takes a security CVE and we fall behind upstream | Slow-burn CVE risk | Out of scope for v0.2 (per non-goal "kernel security hardening beyond minimal CONFIG"); revisit at v0.3. Document the explicit deferral in `m80-ci9i.5` DOCS. |
| R7 | `8250.nr_uarts=0` cmdline change suppresses console output we needed for debugging | When something fails, M80_PHASE_TRACE has no boot-stage detail | DESIGN leaf records the trade: `8250.nr_uarts=1` (one UART) gives us console. The save from `nr_uarts=0` is ~50 ms; not worth the lost diagnostics. **Pin `nr_uarts=1`, not `0`.** |

### 2.2 Existing leaves, extended

- **`m80-ci9i.1` (DESIGN).** **Append `CONFIG_OVERLAY_FS=y` and `CONFIG_OVERLAY_FS_XINO_AUTO=y` to the keep-list** (R1). **Pin `8250.nr_uarts=1`** in the cmdline trim, not `nr_uarts=0` (R7). Effort: **M (1 d, upper 1.5 d)**.
- **`m80-ci9i.2` (kernel build pipeline).** Acceptance extension: deterministic build (R5). Effort: **L (2-3 d, upper 4 d)**.
  - **Rollback:** git revert is insufficient on its own — also delete cached `vmlinux-m80-<sha>.bin` artifacts and revert manifest schema bump. Document the disk-cleanup steps in the bead description.
- **`m80-ci9i.3` (cmdline trim & kernel-kind dispatch).** Effort: **S (4 h)**. Rollback: revert sufficient; per-launch boot args.
- **`m80-ci9i.4` (BENCH).** Expected save: **500-700 ms (high confidence)**. Pre-condition: R1, R2 passed. Effort: **S (3 h)**.
- **`m80-ci9i.5` (DOCS).** Acceptance extension: explicit "deferred to v0.3" line for security-hardening (R6). Effort: **S (1-2 h)**.

### 2.3 New leaf: `m80-ci9i.3b` smoke checkpoint

Between `m80-ci9i.3` IMPL and `m80-ci9i.4` BENCH. Single launch with stripped kernel:

1. Guest reaches userspace and m80-guestd binds the vsock listener.
2. Run `sha256sum /etc/os-release` inside the guest; assert the output (R2 — kernel didn't drop AES-NI).
3. Run a `mount` command and assert overlayfs is the rootfs (R1 — stripped kernel has overlayfs).
4. Capture `phase_12b_ready_accept` time; assert < 400 ms (rough bound; below this we proceed to BENCH; above, we triage before producing N=30 noise).

**Effort:** **S (1-2 h)**.
**Rollback:** N/A.

### 2.4 Cross-epic dependencies (stripped kernel)

- **`m80-ci9i.1` ← `m80-f2zc.5`** (related): kernel config keep-list must include overlayfs symbols. This is the most critical cross-epic constraint in the entire roadmap; if missed, both pivot AND stripped-kernel paths regress simultaneously.
- **`m80-ci9i` does not block `m80-f2zc`** in either direction at the leaf level — but if BOTH are landing, land `m80-f2zc.5` first (verifies stock kernel) so it's clear whether `m80-ci9i`'s build replaces or augments the stock kernel.
- **`m80-rrp.3.4` (snapshot launch path)** consumes whichever kernel is live; should be tested with stripped kernel before BENCH.

### 2.5 `br` commands

```bash
# 1. Extend DESIGN with overlayfs + console trade-off pins.
br update m80-ci9i.1 --notes "EXTEND: keep-list MUST include CONFIG_OVERLAY_FS=y AND CONFIG_OVERLAY_FS_XINO_AUTO=y (R1, gates m80-f2zc). Cmdline: pin 8250.nr_uarts=1, not =0 — preserves console for debugging at cost of ~50ms (R7). Risk register R1-R7 in perf-roadmap-extended.md §2.1."

# 2. Build pipeline rollback note + determinism acceptance.
br update m80-ci9i.2 --notes "rollback: git revert is INSUFFICIENT alone; also delete cached kernels/vmlinux-m80-<sha>.bin and revert manifest SCHEMA_VERSION bump. Acceptance ext: deterministic build (same input -> byte-identical vmlinux); pin Ubuntu 22.04 by digest, KBUILD_BUILD_TIMESTAMP=0, SOURCE_DATE_EPOCH set."

# 3. Confidence-and-savings note on BENCH.
br update m80-ci9i.4 --notes "expected save: 500-700 ms on phase_12b_ready_accept (high confidence; firecracker community 150-300 ms userspace targets). Pre-condition: R1, R2 passed in m80-ci9i.3b smoke."

# 4. DOCS extends with explicit security deferral.
br update m80-ci9i.5 --notes "acceptance ext: explicit 'kernel security hardening deferred to v0.3' line in CHANGELOG; covers R6."

# 5. New smoke leaf.
br create "SMOKE: stripped-kernel boot probe (overlayfs, sha256sum sanity, ready_accept budget)" \
  --type task --priority P2 --parent m80-ci9i \
  --labels active,kernel,smoke,v02 \
  --deps "parent-child:m80-ci9i.3" \
  -d "Single end-to-end launch with stripped kernel, gates m80-ci9i.4 BENCH. Asserts: (1) guestd binds vsock; (2) sha256sum of /etc/os-release matches expected (R2: AES-NI/crypto not dropped); (3) 'mount' shows overlayfs at / (R1: kernel has CONFIG_OVERLAY_FS=y); (4) phase_12b_ready_accept <400 ms (>400 ms triggers diagnostics-first triage). Effort: S (1-2 h). See perf-roadmap-extended.md §2.3."

# 6. Cross-epic dep already filed in §1.6 (m80-ci9i.1 ← m80-f2zc.5).
```

---

## 3. Snapshot/restore — `m80-rrp.3`

**Headline:** Firecracker memory snapshot + warm pool. Bypasses kernel boot entirely on warm path. Expected save: **125-200 ms warm restore (medium confidence)**. AWS published numbers say <200 ms restore; our setup may differ (no I/O hardware acceleration, different host kernel, vsock state survival not yet verified). **The single highest-risk leaf in the whole roadmap.** No production runtime we surveyed (smolvm, kata-containers) wired the Firecracker snapshot REST surface to a working lifecycle.

`m80-rrp.3` already has 6 leaves filed (`.4` through `.9`). Missing pieces: **DESIGN (`.1`)**, **m80-firecracker-client REST methods (`.2`)**, **m80-snapshot capture/restore execution (`.3`)**, a **vsock-survival research leaf (`.0` or new sub)**, the **smoke checkpoint**, and the **warm-pool fill orchestration** (which is partly out-of-scope per the existing `.4`'s description but should be its own bead under `m80-rrp` parent, not `m80-rrp.3`).

### 3.1 Risk register

| # | Failure mode | Detection | Mitigation |
|---|---|---|---|
| R1 | Vsock state does not survive snapshot/restore — guest's in-memory connection is dead, no accept on host | `phase_12b_ready_accept` times out post-restore | Already partially mitigated: host pre-creates UDS at LoadSnapshot time (`m80-rrp.3.8`); guestd redials on EPIPE/ECONNRESET (`m80-rrp.3.9`). New **research leaf** (`m80-rrp.3.0`) explicitly verifies vsock semantics before IMPL — this is high-risk territory. |
| R2 | LoadSnapshot/CreateSnapshot REST endpoints behave differently across firecracker versions | manifest's `firecracker_version` mismatch produces silent corruption | Manifest checked in `m80-snapshot::restore()`; pin firecracker version exactly (already partial — `RestoreMetadata` carries it). DESIGN leaf `m80-rrp.3.1` decides what other compat checks (kernel sha, machine-config) are enforced. |
| R3 | Snapshot taken on overlayfs-rootfs guest captures stale upper-dir refs that break on restore | overlay mount tree is part of the snapshot memory state; vdb is pre-baked at capture time and re-attached at restore — but if vdb is a *different* host file, overlay's in-memory inode refs are wrong | DESIGN leaf must pin: snapshot artifact set includes the **overlay disk file** by reference (path + sha) and restore re-attaches a fresh-but-identical-state overlay (e.g. an `overlay-template.ext4` we keep around per snapshot). Cross-epic dep `m80-rrp.3 ↔ m80-f2zc.4`: snapshotting before storage pivot lands gets you a snapshot of the *old* rootfs-clone architecture, which is throw-away work. **Storage pivot must land first.** |
| R4 | Stale wall-clock in restored VM breaks any process that cares (e.g. exec'd python script with timestamp logic) | Process behavior diverges from a cold-boot baseline | Documented in `m80-rrp.3.7`; "fine for stateless agents, broken for time-critical daemons". Add an explicit `clock_gettime` reset call in m80-guestd's post-restore redial path? Out of scope for v0.2; document as a known limitation. |
| R5 | Snapshot disk artifact is large (memory file ≈ vm_mem_mib MiB) — pool of 16 = 16× memory on disk | disk space pressure | Pool sizing is `m80-rrp.4+` work, not `m80-rrp.3`. Document the per-snapshot footprint in `m80-rrp.3.7` for pool-sizing leaf to consume. |
| R6 | Network state across snapshot: NAT translation table and conntrack entries are host-side and stale | OutboundNat connections fail post-restore | NoEgress is unaffected (no host state). For v0.2, snapshot/restore is **NoEgress only**; OutboundNat compat is `m80-rrp.3.{tbd}` follow-up. Document explicitly in `.7`. |
| R7 | Capture-running pauses the VM mid-exec → in-flight stdout/stderr lost | exec invariants broken | DESIGN: capture preconditions require VM either idle (no exec running) OR caller accepts in-flight loss. `RunningSandbox::capture()` enforces idle by default; force-capture is opt-in. |
| R8 | Snapshot/restore feels great in idle bench, terrible under disk-I/O contention (memory file copy is the long pole) | BENCH `m80-rrp.3.6` loaded-cell shows tail latency >> AWS-published 200 ms | Document tail latency; mitigate at `m80-rrp.4+` (warm pool sizing absorbs latency variance with pre-restored VMs). |

### 3.2 Existing leaves, extended

The existing decomposition is **partial but reasonable** for the parts already filed (`.4` through `.9`). Extensions:

- **`m80-rrp.3.4` (Sandbox::launch_from_snapshot).** Add cross-epic note: storage pivot must land first (R3). Add rollback note: revert is sufficient; no on-disk-state created beyond the per-launch run-dir. Effort: **L (2-3 d, upper 4 d)**.
- **`m80-rrp.3.5` (CLI surface).** Effort: **S-M (4 h - 1 d)**.
- **`m80-rrp.3.6` (BENCH).** Expected save vs cold: **125-200 ms restore (medium confidence — AWS published)**. Pre-condition: R1, R3 mitigations live. Effort: **S (3 h)**.
- **`m80-rrp.3.7` (DOCS).** Add explicit known-limitation entries for R4 (wall-clock), R5 (disk footprint), R6 (NoEgress only). Effort: **S (1-2 h)**.
- **`m80-rrp.3.8` and `.3.9` (vsock pre-create + redial).** Both have explicit dual-side coverage. Effort: each **M (1 d)**. Confidence: medium.

### 3.3 New leaves to file

The chain for `m80-rrp.3` is missing the **early** parts of the lifecycle (DESIGN, REST methods, snapshot crate execution) and a **pre-IMPL research** leaf for the vsock-survival question.

#### `m80-rrp.3.1` — DESIGN

Lock the snapshot architecture decisions in one written design at `docs/design/snapshot-restore.md`:

1. **Capture preconditions.** Does the VM need to be Paused? (Yes, per Firecracker docs — `PUT /vm` `state: Paused` before `PUT /snapshot/create`.) What does `RunningSandbox::capture()` do if exec is in-flight? (Reject by default; opt-in force.)
2. **Compat checks at restore.** Minimum viable set: `firecracker_version` exact match, `kernel_sha256` exact match, `mem_size_mib` match, `vcpu_count` match. NOT enforced: host kernel version, host CPU microcode, CPUID features. Document as "undefined behavior on mismatch" and let the user opt-in to a stricter check via a `--strict-compat` CLI flag (out of scope here, but the design should anticipate the surface).
3. **Vsock semantics across snapshot.** Pre-load this from the research leaf (`m80-rrp.3.0`). If the answer is "vsock state is captured but UDS is not", the host pre-create + guestd redial pattern works (already in `.8` and `.9`). If the answer is something else, this DESIGN must encode the new pattern.
4. **Storage interaction with overlay rootfs.** Per R3: snapshot artifact set includes the overlay disk file by reference (sha + path); restore re-attaches a `*.ext4` of identical state. Decide: do we keep an `overlay-template.ext4` per snapshot, or do we mkfs.ext4 a fresh sparse one at restore time? **Recommendation:** mkfs fresh; the overlay's *contents* should be empty at capture time anyway (capture is taken right after a clean boot, before any exec).
5. **Snapshot REST API stability.** Firecracker docs say snapshot APIs are version-pinned, not stable. Pin a specific firecracker version + capture in `Cargo.toml` workspace.
6. **CLI ergonomics.** Final answer to the `Cmd::Launch --from-snapshot` vs `SandboxConfig.from_snapshot` question per `m80-rrp.3.4` description.

**Acceptance:** doc landed at `docs/design/snapshot-restore.md`; READMEs of `m80-snapshot`, `m80-firecracker-client`, `m80-firecracker`, `m80-cli` reconciled against it.

**Effort:** **M-L (1-3 d, upper 3 d)** — mostly research + decision-locking.

**Dependencies:** none in m80; should land **after** `m80-rrp.3.0` research leaf so DESIGN can encode the answer.

#### `m80-rrp.3.0` — Research: vsock survival across snapshot

Time-boxed investigation: does Firecracker's vsock muxer state survive `CreateSnapshot` + `LoadSnapshot`? Build the smallest possible test: a stock firecracker, a guest that opens a vsock connection and writes a known byte, capture, restore, see what happens host-side and guest-side. Document in `docs/exploration/firecracker-vsock-snapshot.md`.

**Acceptance:** doc landed with three things:
1. Empirical answer (UDS gone but in-VM state survives ⇒ pre-create + redial works; or something else).
2. The firecracker version tested (R2 / R5 informer).
3. Recommendation feeding `m80-rrp.3.1` DESIGN.

**Effort:** **S-M (4 h - 1 d)** — time-boxed; if at 1 day we don't have an answer, escalate to 2 d but no further.

**Dependencies:** none.

#### `m80-rrp.3.2` — IMPL m80-firecracker-client REST methods

`put_snapshot_create` + `put_snapshot_load` REST methods + their config types (`CreateSnapshot`, `LoadSnapshot`). Fixture-server tests covering happy path + 5xx + malformed-response. Currently absent; the existing `.4` IMPL bead assumes they exist.

**Acceptance:**
- `crates/m80-firecracker-client/src/...` exposes both methods.
- Fixture tests pass: happy path, 400 (bad request), 500 (internal error), connection-refused.
- README "Public surface" reflects the additions.

**Effort:** **S (4 h)**. Mechanical against firecracker's documented OpenAPI surface.

**Dependencies:** `m80-rrp.3.1` (DESIGN — pins request shape).

**Rollback:** revert sufficient; no on-disk state.

#### `m80-rrp.3.3` — IMPL m80-snapshot capture/restore execution

Replace `SnapshotError::Deferred` stubs with real plumbing:
- `capture()`: pause VM, call `put_snapshot_create`, write manifest + restore-metadata, verify artifact set sha.
- `restore()`: read manifest, validate compat (per DESIGN), orchestrate `put_snapshot_load`.

Today the schemas + persistence-path helpers are active in v0.1; this leaf wires them to behavior.

**Acceptance:**
- `m80-snapshot::capture()` and `restore()` return `Ok` on happy path; `SnapshotError::Deferred` removed.
- Tests: capture happy-path with mocked client (m80-firecracker-client fixture); restore happy-path; restore with mismatched firecracker_version returns `SnapshotError::CompatMismatch` not `Deferred`.

**Effort:** **L (2-3 d, upper 4 d)** — orchestration + manifest validation logic. The actual REST plumbing is in `.2`; this glues it to the schema layer.

**Dependencies:** `m80-rrp.3.2` (REST methods).

**Rollback:** revert sufficient; persisted snapshot dirs from prior runs become unreadable but harmless (manifest schema unchanged).

#### `m80-rrp.3.5b` — SMOKE: snapshot capture + restore round-trip

Between `.4` (orchestrator) / `.5` (CLI) and `.6` (BENCH). Single round-trip:
1. `m80 launch` a minimal/idle VM, exec `/bin/echo hello`, capture snapshot.
2. `m80 launch --from-snapshot <path>`, exec `/bin/echo hello-from-restore`.
3. Both execs return success; restore launch's `useful_ms` < cold launch's `useful_ms` − 500 ms (lower bound of expected save; if it's not at least this much, triage before BENCH).
4. Capture base file sha256 unchanged (cross-check with storage pivot smoke).
5. Capture overlay disk size at restore stays small (R3 mitigation: fresh mkfs, not stale).

**Effort:** **S (1-2 h)**.
**Rollback:** N/A.

#### `m80-rrp.3.10` — Warm-pool fill orchestration

Out of `m80-rrp.3` proper but on the same chain. Today `m80-rrp.3.4` says "warm-pool management is NOT in scope". This leaf is the next bead in line: maintain N pre-restored VMs ready for instant handoff. Belongs under `m80-rrp` (the warm-pool epic), not `m80-rrp.3`. **File as `m80-rrp.4`** (or whatever next sequential ID). Out of scope for THIS doc except to mention it as the "next thing after `m80-rrp.3` lands".

(We do NOT file `m80-rrp.4` in this pass — it depends on the entirety of `.3` landing first, and pool sizing is its own design exercise.)

### 3.4 Cross-epic dependencies (snapshot/restore)

- **`m80-rrp.3.4` ← `m80-f2zc.4`** (related): snapshotting must consume the post-pivot rootfs architecture (R3). Pre-pivot snapshots are throw-away.
- **`m80-rrp.3.{4,6}` ← `m80-ci9i.4`** (related, weak): bench numbers should be against the live kernel (whichever is current). If both kernel and storage have landed, BENCH is most informative.
- **`m80-rrp` ↔ `m80-qokt.2`** (related, already filed): persistent-VM mode interacts with warm-pool reset semantics; both contribute to multi-turn agent UX.

### 3.5 `br` commands

```bash
# 1. Research leaf (gates DESIGN).
br create "RESEARCH: vsock state survival across firecracker snapshot/restore" \
  --type task --priority P1 --parent m80-rrp.3 \
  --labels active,research,snapshot,vsock,v02 \
  -d "Time-boxed (1 d) investigation: does firecracker vsock muxer state survive CreateSnapshot + LoadSnapshot? Build smallest possible repro: stock firecracker, guest opens vsock connection writes known byte, capture, restore, document host + guest behavior. Output: docs/exploration/firecracker-vsock-snapshot.md with empirical answer, firecracker version tested, recommendation feeding m80-rrp.3.1 DESIGN. Effort: S-M (4 h - 1 d, upper 2 d). See perf-roadmap-extended.md §3.3."

# 2. DESIGN leaf.
br create "DESIGN: snapshot-restore architecture (preconditions, compat, vsock semantics, overlay interaction)" \
  --type task --priority P1 --parent m80-rrp.3 \
  --labels active,design,snapshot,v02 \
  -d "Lock decisions in docs/design/snapshot-restore.md: (1) capture preconditions (Paused before CreateSnapshot; in-flight exec policy); (2) restore compat-check minimum (firecracker_version, kernel_sha256, mem_size_mib, vcpu_count) and which checks are advisory; (3) vsock semantics (pre-load from research leaf); (4) overlay interaction — fresh mkfs at restore, NOT stale overlay-template; (5) firecracker version pin in workspace Cargo.toml; (6) CLI ergonomics — Launch --from-snapshot vs SandboxConfig.from_snapshot. Acceptance: doc landed; m80-snapshot, m80-firecracker-client, m80-firecracker, m80-cli READMEs reconciled. Effort: M-L (1-3 d). Deps: research leaf above. See perf-roadmap-extended.md §3.3."

# 3. m80-firecracker-client REST methods.
br create "IMPL m80-firecracker-client: put_snapshot_create + put_snapshot_load REST methods + config types" \
  --type task --priority P1 --parent m80-rrp.3 \
  --labels active,firecracker-client,snapshot,v02 \
  -d "Add put_snapshot_create + put_snapshot_load REST methods + CreateSnapshot/LoadSnapshot config types. Fixture-server tests: happy path, 400, 500, connection-refused. README 'Public surface' reflects additions. Currently absent; m80-rrp.3.4 IMPL assumes they exist. Effort: S (4 h). Deps: DESIGN leaf. Rollback: revert sufficient. See perf-roadmap-extended.md §3.3."

# 4. m80-snapshot execution.
br create "IMPL m80-snapshot: capture()/restore() execution lane (replace SnapshotError::Deferred)" \
  --type task --priority P1 --parent m80-rrp.3 \
  --labels active,snapshot,v02 \
  -d "Replace SnapshotError::Deferred stubs. capture(): pause VM, call put_snapshot_create, write manifest + restore-metadata, verify artifact-set sha. restore(): read manifest, validate compat per DESIGN, orchestrate put_snapshot_load. Tests: capture/restore happy paths with mocked m80-firecracker-client; mismatched firecracker_version returns SnapshotError::CompatMismatch (not Deferred). Effort: L (2-3 d, upper 4 d). Deps: m80-firecracker-client REST methods. Rollback: revert sufficient. See perf-roadmap-extended.md §3.3."

# 5. Wire deps for the existing .4 leaf.
br update m80-rrp.3.4 --notes "EXTEND: deps include the new m80-snapshot capture/restore execution leaf above (replaces stub assumption). Cross-epic: requires m80-f2zc.4 (storage pivot) landed — snapshot needs post-pivot rootfs architecture (R3). Rollback: revert sufficient; per-launch run-dir."
br update m80-rrp.3.6 --notes "expected save: 125-200 ms warm restore vs cold (medium confidence; AWS published <200 ms). Pre-condition: vsock survival research (m80-rrp.3.0 equivalent), DESIGN, IMPL leaves landed. Loaded-cell tail latency informs m80-rrp.4 pool sizing (R8)."
br update m80-rrp.3.7 --notes "EXTEND: explicit known-limitation entries for R4 (stale wall-clock; out of scope for v0.2), R5 (disk footprint per snapshot ~= vm_mem_mib MiB), R6 (NoEgress only for v0.2; OutboundNat compat is followup)."

# 6. Smoke checkpoint between IMPL and BENCH.
br create "SMOKE: snapshot capture + restore round-trip probe (cold-vs-restore delta floor, base sha, overlay size)" \
  --type task --priority P1 --parent m80-rrp.3 \
  --labels active,snapshot,smoke,v02 \
  -d "Single round-trip gating m80-rrp.3.6 BENCH. Asserts: (1) m80 launch + capture succeeds; (2) m80 launch --from-snapshot + exec succeeds; (3) restore useful_ms < cold useful_ms - 500 ms (lower-bound floor; <500 ms delta triggers diagnostics-first triage); (4) base file sha256 unchanged; (5) overlay disk size remains small (R3 mitigation: fresh mkfs, not stale). Effort: S (1-2 h). Deps: m80-rrp.3.4, .3.5. See perf-roadmap-extended.md §3.3."
```

Note on IDs: the `br create` calls above will get auto-generated suffixes like `m80-rrp.3.10`, `.3.11`, etc. The numbers don't matter; the parent linkage and dep chain do.

---

## 4. Persistent VM — `m80-qokt.2`

**Headline:** relax the one-exec-per-VM contract so multi-turn agent sessions don't pay cold-boot latency between turns. The expected save **per turn after the first** is ~the entire cold-launch budget (currently 1.9 s, post-chain ~200 ms). For a 10-turn session: 1.8 s × 9 turns saved if cold path remains the baseline; ~0 saved per turn if warm-pool already covers the gap. **Confidence: high on mechanism, low on net-positive impact** *if* warm pool is already in place — persistent VM is most valuable for sessions that are explicitly stateful (file scratch, env vars, shell history), where a warm-pool VM is wrong because it has no continuity.

`m80-qokt.2` today is an epic with no sub-leaves filed. Decompose into 6 leaves.

### 4.1 Risk register

| # | Failure mode | Detection | Mitigation |
|---|---|---|---|
| R1 | `&mut self` API mutation breaks every call site at once; library and CLI consumers all need updates simultaneously | compile fails | Sequence: API leaf lands first; consumers (CLI, m80-adapter when it exists) update in same diff. Pre-1.0 internal API per CLAUDE.md "no shims". |
| R2 | Sequential exec on one VM accumulates state (filesystem, /tmp, env) — turn N sees turn N-1's leftovers | feature, not bug, IF caller wants persistence; bug if caller wanted clean | Document in DESIGN that persistent VMs are **explicitly** stateful; reset semantics live under `m80-rrp.1` (reset-evidence vocabulary). Clean-VM cases use the warm pool (`m80-rrp.4+`), not persistent VMs. |
| R3 | Cancellation contract during in-flight exec is unclear — caller's drop of an exec future, what happens to the guest process | guest process becomes orphan; future exec sees zombies | Explicit cancellation leaf (`m80-qokt.2.4`). Drop-without-await sends SIGKILL via vsock; guestd reaps. |
| R4 | Idle persistent VM leaks resources — no idle timeout means N stale VMs forever | host RAM exhausted at scale | Idle-timeout leaf (`m80-qokt.2.5`). Default: 5 min idle → graceful shutdown. Configurable via `SandboxConfig`. |
| R5 | Bench shows turn-to-turn latency is *not* ~0 because of vsock contention or guestd state thrashing | BENCH P50 of turn N>1 still > 50 ms | Investigate; not a launch issue, an exec-loop issue. Mitigation tied to `m80-vsock` perf, not this epic. |
| R6 | A single broken exec leaves the VM in a bad state, but persistent-VM contract says "keep it"; caller has no signal | next exec returns garbage | DESIGN must address: does `RunningSandbox::exec` mark the sandbox poisoned on guestd-side panic? If yes, what's the caller's recovery (re-launch)? Document. |

### 4.2 New leaves to file

#### `m80-qokt.2.1` — DESIGN: persistent-state lifecycle, cancellation, idle-timeout, poisoning

Lock the contract:
1. `RunningSandbox::exec(&mut self, req) -> Result<ExecResult, ...>` — sequential, one in-flight at a time.
2. **Cancellation:** caller drops future ⇒ guestd receives `Cancel(request_id)` over vsock; guestd SIGKILLs the in-flight process; sends final ack. If host doesn't get ack within 2 s, the VM is poisoned.
3. **Idle timeout:** default 5 min from last `exec()` return; on expiry, graceful shutdown via existing `ShutdownRequest` path. Configurable via `SandboxConfig::idle_timeout`.
4. **Poisoning:** if guestd panics or fails to ack a cancel, `RunningSandbox` transitions to Poisoned; subsequent `exec()` returns `FcError::SandboxPoisoned`; caller must drop and re-launch.
5. **State persistence guarantees:** filesystem (overlay), env-via-shell-history, /tmp, ALL persist across exec calls. Document explicitly — this is the feature.
6. **Interaction with reset-evidence vocabulary (`m80-rrp.1`):** orthogonal. Persistent-VM mode does not consume reset evidence; it's an opt-out from reset entirely. Warm-pool VMs consume reset evidence; persistent VMs do not.

**Acceptance:** `docs/design/persistent-vm.md` landed; `m80-firecracker` README's "Public surface" reflects `&mut self`.
**Effort:** **M (1 d, upper 1.5 d)**.
**Deps:** `m80-qokt` (parent epic).

#### `m80-qokt.2.2` — IMPL: `RunningSandbox::exec(&mut self)` API mutation

Change the receiver. Update all in-tree consumers (CLI, tests).
**Acceptance:**
- `crates/m80-firecracker/src/lib.rs` (or wherever): `RunningSandbox::exec` takes `&mut self`.
- `m80-cli` updated.
- Existing single-exec tests still pass.
- README "Public surface" updated.
**Effort:** **S (4 h)**. Mechanical refactor.
**Rollback:** revert sufficient.
**Deps:** `m80-qokt.2.1`.

#### `m80-qokt.2.3` — TESTS: sequential exec on a single VM

New `crates/m80-firecracker/tests/persistent_state.rs`. Test cases (one `#[test]` each per CLAUDE.md):
1. Two sequential `echo` execs both succeed; second sees first's filesystem write at `/tmp/x`.
2. Three sequential execs interleaving stdin (tracks bytestream isolation).
3. Sequential exec after long idle (1 s) still works.
4. Sequential exec after exec failure (exit 1) still works on the same VM.
5. Captured doc at `docs/behaviors/lifecycle/persistent-state.md`.
**Effort:** **M (1 d)**.
**Rollback:** revert sufficient.
**Deps:** `m80-qokt.2.2`.

#### `m80-qokt.2.4` — IMPL + TESTS: cancellation contract

Wire the Cancel envelope in `m80-proto` (if not already), guestd handler, host-side drop handler.
**Acceptance:**
- `crates/m80-firecracker/tests/cancellation.rs`: caller-drops-future ⇒ guest process is killed (verified by exec-after returns expected fresh state); 2 s timeout puts VM in Poisoned; subsequent exec returns `SandboxPoisoned`.
- `m80-proto` Cancel envelope (or already exists; verify).
- `docs/behaviors/lifecycle/exec-cancellation.md`.
**Effort:** **M-L (1-2 d, upper 3 d)**.
**Rollback:** revert sufficient (no persisted state).
**Deps:** `m80-qokt.2.3`.

#### `m80-qokt.2.5` — IMPL + TESTS: idle timeout

`SandboxConfig::idle_timeout: Option<Duration>` (default Some(5 min) for persistent-VM-mode runs). On expiry, host issues `ShutdownRequest`; existing graceful-stop path consumes it.
**Acceptance:**
- New field in `SandboxConfig`.
- Test: short idle_timeout (1 s); VM transitions to Stopped after 1 s of no exec.
- Test: continual exec keeps VM alive past timeout boundary.
- `docs/behaviors/lifecycle/idle-timeout.md`.
**Effort:** **M (1 d)**.
**Rollback:** revert sufficient.
**Deps:** `m80-qokt.2.4`.

#### `m80-qokt.2.6` — BENCH + DOCS

Bench: P50 of N=30 sequential execs on a single VM (turn-to-turn latency); compare to N=30 cold launches. Document the difference. Doc work: `docs/behaviors/lifecycle/persistent-state.md` (already required by `.3`); CHANGELOG entry; README updates for `m80-firecracker` (one-exec-per-VM line removed; multi-exec contract stated).
**Expected save:** ~ entire cold-launch budget per turn after the first. **High confidence on mechanism**; low confidence on whether this beats warm-pool re-allocation in real workloads. **The bench is the source of truth.**
**Effort:** **S-M (4 h - 1 d)**.
**Rollback:** N/A.
**Deps:** `m80-qokt.2.5`.

### 4.3 Cross-epic dependencies (persistent VM)

- **`m80-qokt.2` ↔ `m80-rrp` (related, already filed):** persistent VM and warm-pool are the two halves of multi-turn agent UX. Persistent VM is for stateful sessions; warm pool is for clean stateless sessions.
- **`m80-qokt.2` ↔ `m80-qokt.1` (in-tree adapter):** the m80-adapter is the natural caller of `RunningSandbox::exec(&mut self)`. Adapter work shouldn't block persistent-VM IMPL but should land in the same diff if both are landing in v0.2.
- **`m80-qokt.2` ↔ `m80-f2zc` (no dep):** orthogonal. Storage pivot's overlay rootfs naturally supports per-VM persistence — persistent-VM mode just means the same overlay file is reused turn after turn instead of being torn down. No code-level dep.
- **`m80-qokt.2` ↔ `m80-rrp.1` reset evidence:** documented as orthogonal in DESIGN; no dep.

### 4.4 `br` commands

```bash
# 1. DESIGN leaf.
br create "DESIGN: persistent-state VM lifecycle (mut-self exec, cancellation, idle-timeout, poisoning)" \
  --type task --priority P1 --parent m80-qokt.2 \
  --labels active,design,lifecycle,v02 \
  -d "Lock contract in docs/design/persistent-vm.md: (1) RunningSandbox::exec(&mut self, req); (2) cancellation: caller-drop -> Cancel envelope -> guestd SIGKILL -> 2s timeout -> Poisoned; (3) idle timeout default 5 min, configurable; (4) poisoning: failed cancel-ack puts VM in Poisoned; subsequent exec returns SandboxPoisoned; (5) state persistence guarantees: filesystem (overlay), env-via-shell-history, /tmp persist across exec; (6) orthogonal to m80-rrp.1 reset-evidence (persistent VM is opt-out from reset). Acceptance: doc landed; m80-firecracker README updated. Effort: M (1 d, upper 1.5 d). See perf-roadmap-extended.md §4.2."

# 2. API mutation IMPL.
br create "IMPL m80-firecracker: RunningSandbox::exec takes &mut self; update CLI consumers" \
  --type task --priority P1 --parent m80-qokt.2 \
  --labels active,lifecycle,v02 \
  -d "Change RunningSandbox::exec receiver to &mut self. Update m80-cli; existing single-exec tests still pass. README 'Public surface' updated. Effort: S (4 h). Rollback: revert sufficient. Deps: DESIGN leaf above. See perf-roadmap-extended.md §4.2."

# 3. Sequential-exec tests.
br create "TESTS: sequential exec on one VM (filesystem, stdin, idle, post-failure)" \
  --type task --priority P1 --parent m80-qokt.2 \
  --labels active,lifecycle,tests,v02 \
  -d "crates/m80-firecracker/tests/persistent_state.rs with 4+ #[test] fns: (1) two sequential echo execs, second sees first's /tmp write; (2) three execs with stdin; (3) sequential after 1s idle; (4) sequential after exec failure (exit 1). Captured doc at docs/behaviors/lifecycle/persistent-state.md. Effort: M (1 d). Deps: API mutation leaf. See perf-roadmap-extended.md §4.2."

# 4. Cancellation contract.
br create "IMPL + TESTS: exec cancellation contract during persistent session" \
  --type task --priority P1 --parent m80-qokt.2 \
  --labels active,lifecycle,proto,v02 \
  -d "Wire Cancel envelope in m80-proto (if absent), guestd handler, host-side caller-drop. Tests at crates/m80-firecracker/tests/cancellation.rs: caller drops future -> guest process killed (post-cancel exec sees fresh state); 2s ack-timeout transitions VM to Poisoned; subsequent exec returns SandboxPoisoned. Doc at docs/behaviors/lifecycle/exec-cancellation.md. Effort: M-L (1-2 d, upper 3 d). Deps: sequential-exec tests leaf. See perf-roadmap-extended.md §4.2."

# 5. Idle timeout.
br create "IMPL + TESTS: idle-timeout for persistent VMs (default 5 min, configurable)" \
  --type task --priority P1 --parent m80-qokt.2 \
  --labels active,lifecycle,v02 \
  -d "Add SandboxConfig::idle_timeout: Option<Duration>; default Some(5 min) for persistent-mode runs. On expiry, host issues ShutdownRequest via existing graceful-stop path. Tests: short idle_timeout (1s) transitions VM to Stopped; continual exec keeps VM alive past timeout. Doc at docs/behaviors/lifecycle/idle-timeout.md. Effort: M (1 d). Deps: cancellation leaf. See perf-roadmap-extended.md §4.2."

# 6. BENCH + DOCS.
br create "BENCH + DOCS: persistent-VM turn-to-turn latency vs cold launch" \
  --type task --priority P1 --parent m80-qokt.2 \
  --labels active,lifecycle,bench,docs,v02 \
  -d "P50 of N=30 sequential execs on a single VM (turn-to-turn) compared to N=30 cold launches. Document delta in docs/behaviors/lifecycle/persistent-state.md. CHANGELOG entry. m80-firecracker README updates: remove one-exec-per-VM line, state multi-exec contract. Expected: ~entire cold-launch budget saved per turn after first; high confidence on mechanism, low on whether it beats warm-pool re-alloc in real workloads — bench is source of truth. Effort: S-M (4 h - 1 d). Deps: idle-timeout leaf. See perf-roadmap-extended.md §4.2."
```

---

## 5. Cross-epic dependency graph (ASCII)

```
                  ┌───────────────────────────────────────────────────────────┐
                  │  CRITICAL CROSS-EPIC EDGE (R1 of m80-ci9i + R3 of m80-f2zc)│
                  │  m80-f2zc.5  ──── related ────►  m80-ci9i.1               │
                  │  (CONFIG_OVERLAY_FS=y in stripped kernel keep-list)        │
                  └───────────────────────────────────────────────────────────┘

  ┌──────────────────────── STORAGE PIVOT (m80-f2zc) ─────────────────────────┐
  │                                                                            │
  │   m80-f2zc.1  DESIGN  (risk register R1-R7)                                │
  │      │                                                                     │
  │      ├──► m80-f2zc.2  IMPL m80-storage (Rootfs::prepare; delete clone)    │
  │      │       │                                                             │
  │      │       └──► m80-f2zc.3  IMPL m80-firecracker (3-drive PUT)          │
  │      │                │                                                    │
  │      │                └──► m80-f2zc.6  TESTS                              │
  │      │                                                                     │
  │      ├──► m80-f2zc.5  KERNEL CONFIG_OVERLAY_FS=y (gates m80-ci9i.1) ───┐  │
  │      │       │                                                          │  │
  │      │       └──► m80-f2zc.4  IMPL m80-guestd (overlayfs + pivot_root) │  │
  │      │                │                                                 │  │
  │      │                ├──► m80-f2zc.5b  ★ SMOKE CHECKPOINT ★           │  │
  │      │                │                                                 │  │
  │      │                └──► m80-f2zc.7  BENCH (700-770 ms save, high)   │  │
  │      │                            │                                     │  │
  │      │                            └──► m80-f2zc.8  DOCS                │  │
  │      │                                                                  │  │
  └──────│──────────────────────────────────────────────────────────────────│──┘
         │                                                                  │
         └─── related-to ─────────────► m80-rrp.3.4  (storage pivot is needed
                                                     before snapshot wires up)
                                                                            │
  ┌──────────────────────── STRIPPED KERNEL (m80-ci9i) ─────────────────────│──┐
  │                                                                          │   │
  │   m80-ci9i.1  DESIGN  ◄─── (related-from m80-f2zc.5 for overlayfs)  ────┘   │
  │      │                                                                       │
  │      ├──► m80-ci9i.2  IMPL m80-image-build (kernel build pipeline)          │
  │      │                                                                       │
  │      └──► m80-ci9i.3  IMPL m80-firecracker (cmdline trim)                   │
  │             │                                                                │
  │             ├──► m80-ci9i.3b  ★ SMOKE CHECKPOINT ★                         │
  │             │                                                                │
  │             └──► m80-ci9i.4  BENCH (500-700 ms save, high)                  │
  │                       │                                                      │
  │                       └──► m80-ci9i.5  DOCS                                  │
  │                                                                              │
  └──────────────────────────────────────────────────────────────────────────────┘

  ┌──────────────────────── SNAPSHOT/RESTORE (m80-rrp.3) ───────────────────────┐
  │                                                                              │
  │   m80-rrp.3.0  RESEARCH (vsock survival)                                     │
  │      │                                                                       │
  │      └──► m80-rrp.3.1  DESIGN  (compat, vsock, overlay, REST API pin)       │
  │             │                                                                │
  │             ├──► m80-rrp.3.2  m80-firecracker-client REST methods           │
  │             │       │                                                        │
  │             │       └──► m80-rrp.3.3  m80-snapshot capture/restore execution│
  │             │                                                                │
  │             ├──► m80-rrp.3.4  IMPL m80-firecracker  ◄──┐ (related-from      │
  │             │                                          │  m80-f2zc.4)        │
  │             ├──► m80-rrp.3.5  IMPL m80-cli                                  │
  │             ├──► m80-rrp.3.8  vsock UDS pre-create                          │
  │             └──► m80-rrp.3.9  guestd vsock redial                           │
  │                       │                                                      │
  │                       ├──► m80-rrp.3.5b  ★ SMOKE CHECKPOINT ★              │
  │                       │                                                      │
  │                       └──► m80-rrp.3.6  BENCH (125-200 ms warm, medium)    │
  │                                  │                                          │
  │                                  └──► m80-rrp.3.7  DOCS                    │
  │                                                                              │
  └──────────────────────────────────────────────────────────────────────────────┘

  ┌──────────────────────── PERSISTENT VM (m80-qokt.2) ─────────────────────────┐
  │   (orthogonal to latency chain; no perf-chain dep)                           │
  │                                                                              │
  │   m80-qokt.2.1  DESIGN                                                       │
  │      │                                                                       │
  │      └──► m80-qokt.2.2  IMPL &mut self exec                                 │
  │             │                                                                │
  │             └──► m80-qokt.2.3  TESTS sequential exec                        │
  │                       │                                                      │
  │                       └──► m80-qokt.2.4  cancellation contract              │
  │                                  │                                           │
  │                                  └──► m80-qokt.2.5  idle-timeout            │
  │                                            │                                 │
  │                                            └──► m80-qokt.2.6  BENCH + DOCS │
  │                                                                              │
  └──────────────────────────────────────────────────────────────────────────────┘

  ★ = post-IMPL smoke checkpoint, gates BENCH; failure here triggers
       diagnostics-first triage per CLAUDE.md.
```

**Critical-path latency chain to sub-200 ms warm:**
`m80-f2zc.{1→2,3,4,5,5b,6}` → land 700-770 ms save (cold ≈ 1.1 s).
Then `m80-ci9i.{1→2,3,3b,4}` → land 500-700 ms save (cold ≈ 400-600 ms).
Then `m80-rrp.3.{0,1,2,3,4,5,8,9,5b,6}` → land snapshot-restore (warm ≈ 200 ms; cold ≈ unchanged).

**Persistent VM is orthogonal**, lands any time after `m80-qokt.1` (in-tree adapter).

**Wire-protocol epics (`m80-6zim`, `m80-5vha`)** are explicitly out of scope for this doc; tracked in `docs/planning/wire-features-bead-plan.md`.

---

## 6. Single-page summary table

| Epic | Existing leaves | New leaves to file | Expected save | Confidence | Critical cross-epic dep |
|---|---|---|---|---|---|
| `m80-f2zc` storage pivot | 8 (.1-.8) | 1 smoke (.5b) | 700-770 ms cold | High | gates `m80-ci9i.1` (overlayfs symbol) |
| `m80-ci9i` stripped kernel | 5 (.1-.5) | 1 smoke (.3b) | 500-700 ms cold | High | needs `m80-f2zc.5` |
| `m80-rrp.3` snapshot/restore | 6 (.4-.9) | 5 (research, design, REST, exec, smoke) | 125-200 ms warm vs cold | Medium | needs `m80-f2zc.4` |
| `m80-qokt.2` persistent VM | 0 | 6 (design, mut-self, tests, cancel, idle, bench) | full cold budget per turn after first | High mech / Low workload | none |

**Total new leaves to file: 13.**
**Total updates to existing leaves: ~10** (notes + cross-epic deps).
**Total effort, conservative upper-bound (sum of upper bounds):** ~22-27 person-days for everything.

---

## 7. Closing notes

The discipline we want this document to encode:

1. **Each leaf has a smoke checkpoint between IMPL and BENCH.** Failure at smoke triggers diagnostics-first triage (CLAUDE.md), not a hypothesis-first scramble. We have lost an hour to that anti-pattern recently and the fix is structural: gate BENCH on smoke.
2. **Effort is a range with the upper bound visible.** Optimistic point estimates have hurt; the upper bound is the bid we accept.
3. **Savings have confidence.** "High" because we have point measurements (storage pivot's 727 ms; stripped kernel's 500-700 ms is firecracker community-standard). "Medium" because AWS published it but our setup may differ (snapshot/restore). "Low/Unknown" only when warranted.
4. **Cross-epic dependencies are explicit edges in the graph.** The single most important edge — `m80-f2zc.5 → m80-ci9i.1` (overlayfs symbol in stripped kernel) — is now belted-and-suspendered: storage-pivot DESIGN risk-register names it; stripped-kernel DESIGN keep-list names it; they cross-link.
5. **Rollback strategy is part of every IMPL leaf.** Most are "git revert is sufficient"; the exceptions (`m80-f2zc.4` requires image rebuild; `m80-ci9i.2` requires cache cleanup + manifest schema revert) are flagged.

When this plan is executed, after each epic lands the bench harness re-run should confirm the expected delta — if it doesn't, the smoke checkpoint should already have caught the failure and we're in diagnostics-first triage, not BENCH-noise hunting.

---

## 8. Aspirational ID → actual ID map (filed 2026-05-05)

`br create` auto-generates suffix IDs; the names used throughout this document
are aspirational. Substitute as follows when reading:

| Aspirational name | Actual bead ID | What it is |
|---|---|---|
| `m80-f2zc.5b` | **`m80-f2zc.9`** | storage-pivot smoke checkpoint |
| `m80-ci9i.3b` | **`m80-ci9i.6`** | stripped-kernel smoke checkpoint |
| `m80-rrp.3.0` (research) | **`m80-rrp.3.10`** | vsock-survival research |
| `m80-rrp.3.1` (DESIGN) | **`m80-rrp.3.11`** | snapshot-restore architecture |
| `m80-rrp.3.2` (REST) | **`m80-rrp.3.12`** | m80-firecracker-client REST methods |
| `m80-rrp.3.3` (exec) | **`m80-rrp.3.13`** | m80-snapshot capture/restore execution |
| `m80-rrp.3.5b` (smoke) | **`m80-rrp.3.14`** | snapshot round-trip smoke |
| `m80-qokt.2.1` DESIGN | `m80-qokt.2.1` ✓ | (matches aspirational) |
| `m80-qokt.2.2` mut-self | `m80-qokt.2.2` ✓ | (matches aspirational) |
| `m80-qokt.2.3` tests | `m80-qokt.2.3` ✓ | (matches aspirational) |
| `m80-qokt.2.4` cancel | `m80-qokt.2.4` ✓ | (matches aspirational) |
| `m80-qokt.2.5` idle | `m80-qokt.2.5` ✓ | (matches aspirational) |
| `m80-qokt.2.6` bench+docs | `m80-qokt.2.6` ✓ | (matches aspirational) |

**Cross-epic deps wired** (visible via `br dep list`):
- `m80-ci9i.1` → `m80-f2zc.5` (related — overlayfs symbol must be in stripped-kernel keep-list)
- `m80-rrp.3.4` → `m80-f2zc.4` (related — snapshot needs post-pivot rootfs architecture)
- `m80-rrp.3.4` → `m80-rrp.3.13` (blocks — orchestrator depends on m80-snapshot exec)
- `m80-qokt.2.4` → `m80-5vha` (related — shared Cancel envelope)
- `m80-qokt.2.{2,3,4,5,6}` chain — sequential `blocks:` deps so the chain executes in order
- Smoke checkpoints block their respective BENCH leaves (`m80-f2zc.7 → m80-f2zc.9`,
  `m80-ci9i.4 → m80-ci9i.6`, `m80-rrp.3.6 → m80-rrp.3.14`)
- `m80-rrp.3.{11,12,13}` chain (DESIGN → REST → exec) — sequential `blocks:`

Wire-protocol epics (`m80-5vha`, `m80-6zim`) **were also annotated** with the
cancellation cross-ref note even though they're out of scope for this doc — so
whichever Cancel-envelope work lands first, the other consumes the existing
contract rather than re-designing it.
