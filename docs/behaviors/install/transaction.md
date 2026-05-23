# Install Transaction

Behavior bead: `m80-o3uh9.3`.

The public Linux install surface is the release installer:

<!-- m80:freshness-status start -->
Public installer status: public proof green for `v0.2.11`.
<!-- m80:freshness-status end -->

```sh
curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```

Automation can pin the same surface:

```sh
curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh
```

`latest` is only a resolver. It must become one stable `vMAJOR.MINOR.PATCH`
tag before versioned install logic runs. Pinned installs use that concrete tag
directly and reject mutable latest bundle URLs, prerelease tags, foreign
repositories, raw branch URLs, and path-shaped release assets.

The installer verifies release material before mutating active host state:

- release asset index and selected tuple;
- selected bundle checksum and bundle metadata;
- public `SHA256SUMS`;
- versioned `install.sh`;
- bootstrap selector;
- build manifest;
- release-integrity predicate;
- GitHub Artifact Attestation bundle and normalized metadata;
- extracted `bin/m80` release identity.

Privilege is only for final host writes. The shell handoff uses an explicit
environment allowlist and invokes the extracted release `bin/m80`, not an
ambient `PATH` binary. Ambient `M80_*` overrides do not select release inputs.

The active pointer flips last. Failed verification, failed profile/config
write, stale proof cache, interrupted install, smoke-gate failure, or
active-pointer failure leaves the previous active release, generated profile,
config, proof cache, and selected version unchanged. Same-version reinstall is
an idempotent verified no-op when bytes and proof material match; changed bytes
or proof material fail closed with an explicit repair command. Downgrades,
prereleases, yanked, and below-safety-floor targets are refused unless a
separate explicit policy names the allowed action.

`m80 install-status` is the local repair view. It rechecks active bytes,
installed metadata, profile/config selection, host-binaries manifest, proof
cache, and latest freshness metadata without mutating the install root, then
prints one next command when a safe repair is known.

Detailed captures:

- [`docs/behaviors/release/installer-input-contract.md`](../release/installer-input-contract.md)
- [`docs/behaviors/release/stable-latest-bootstrap-handoff.md`](../release/stable-latest-bootstrap-handoff.md)
- [`docs/behaviors/release/install-invocation.md`](../release/install-invocation.md)
- [`docs/behaviors/release/install-finalization-transaction.md`](../release/install-finalization-transaction.md)
- [`docs/behaviors/release/install-state.md`](../release/install-state.md)
- [`docs/behaviors/release/downgrade-refusal.md`](../release/downgrade-refusal.md)
