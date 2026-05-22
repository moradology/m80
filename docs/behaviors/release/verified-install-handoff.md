# Verified Install Handoff

The fastest public install command may remain a `curl | sudo sh` convenience
path, but release automation and operators can verify the installer bytes before
privilege is involved. The verified handoff path downloads these release assets
into a temporary directory:

```text
install.sh
install.sh.sha256
m80-release-integrity.json
m80-release-integrity.attestation.jsonl
m80-release-attestation.json
```

`scripts/verify-install-handoff.py` verifies the local `install.sh` bytes
against `install.sh.sha256`, requires matching `installer` and
`checksum-sidecar` subjects in `m80-release-integrity.json`, then verifies the
predicate with the trusted m80 release policy and GitHub Artifact Attestations.
Only after that does it print the local `sudo sh <tmp>/install.sh` handoff
command.

The verifier fails before printing the sudo handoff when the installer is
tampered, the checksum is wrong, signature/provenance material is missing, the
requested tag differs from the predicate tag, or the native verifier cannot
bind the attestation bundle to the predicate. Its human output includes the
resolved release tag, source commit,
`install_sh_sha256`, and the verified asset names so CI logs show exactly which
local bytes are about to cross the privilege boundary.

Release CI validates the published assets with this verifier after
re-downloading the GitHub Release payload. That check is not a real-KVM smoke or
an install-root mutation; it is the pre-root integrity gate that lets the later
privileged invocation consume local verified bytes instead of unchecked network
stdout.

Coverage:

- `scripts/test-install-handoff.py`
- `.github/workflows/release-artifacts.yml`
