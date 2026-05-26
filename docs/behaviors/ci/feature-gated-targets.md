# Feature-Gated CI Targets

Behavior capture for bead `m80-6uc4k.2`.

## Contract

CI must compile and run tests for targets that Cargo hides behind
`required-features`. A green workspace test run is not sufficient when a crate
declares binaries or integration tests that only exist with explicit features.

The ordinary CI workflow runs these feature-gated checks:

- `cargo test -p m80-attack-runner --features malicious-artifact -- --test-threads=2`
- `cargo test -p m80-guestd-malicious --features malicious-artifact -- --test-threads=2`

CI also checks feature-set extremes so default-feature drift and all-feature
compile failures are visible:

- `cargo check --workspace --no-default-features`
- `cargo check --workspace --all-features`

The specs maintenance scripts are wired into CI as syntax and shell lint gates.
They are not executed in GitHub-hosted CI because `specs/03-validate.sh`
requires local `br`/`bv` tracker tooling and `specs/04-deps.sh` mutates tracker
edges. CI still fails if those scripts stop parsing or violate shellcheck.

## Regression Pin

Enabling the `m80-guestd-malicious` feature-gated test target exposed a hidden
compile failure in `response_type_mismatch_echoes_request_id_with_wrong_payload_shape`.
The request id is now passed as `&str` instead of an ambiguous `.into()`, so the
`Envelope::with_request_id` type parameter is inferred from the payload rather
than from the request id argument.

## Verification

- `cargo check --workspace --no-default-features`
- `cargo check --workspace --all-features`
- `cargo test -p m80-attack-runner --features malicious-artifact -- --test-threads=2`
- `cargo test -p m80-guestd-malicious --features malicious-artifact -- --test-threads=2`
- `cargo test -p m80-observability -- --test-threads=2`
- `python3 scripts/lint-github-workflows.py`
- `python3 scripts/run-actionlint.py --workflow-dir .github/workflows`
- `bash -n specs/03-validate.sh specs/04-deps.sh`
- `shellcheck -x -s bash specs/03-validate.sh specs/04-deps.sh`
