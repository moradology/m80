# Freshness Network Bounds

The scheduled hostless freshness lane uses one bounded fetch policy for the
public latest install surface before it decides that docs and latest are still
live:

- GitHub latest release metadata, including the guard re-read used to detect a
  tag switch;
- the documented `releases/latest/download/install.sh` URL;
- the pinned `releases/download/<tag>/install.sh` URL for the resolved stable
  tag;
- every installer-consumed public asset URL emitted by
  `scripts/stable_latest_bootstrap.py`, including the asset index, selector,
  release-integrity predicate, attestation bundle, normalized attestation
  metadata, release build manifest, and selected bundle;
- concrete docs-linked release install URLs discovered by
  `scripts/quickstart_snippets.py`.

`scripts/release_freshness.py` applies the same bounds used by the stable
latest bootstrapper and shell installer: 10 second connect timeout, 120 second
total timeout, two retries, and a one second retry delay. Fixture mode remains
network-free for release-script tests; URL mode records the checked URL set in a
machine-readable proof summary.

Failures are intentionally specific. Timeout, DNS/connect failure, HTTP
failure, redirect-loop, and partial-download cases name the URL, release tag
when known, asset name, freshness role, curl exit code, and sources such as the
release URL contract or `docs:<path>:<line>`. That output is the repair handle:
stale public URL, missing release asset, private/auth-required response, or a
docs snippet that points at the wrong install surface.

Freshness failures are bounded CI failures, not hung release jobs. They do not
prove real-KVM execution; the privileged freshness lane owns that substrate
proof separately. Until `m80-o3uh9.21.2` lands the policy is conservative:
freshness failure blocks a green freshness status and the next release/latest
promotion, does not page automatically, and should file or update one freshness
bead after a single retry rules out a transient network blip.
