# Stable Latest Bootstrap Preflight

Bead: `m80-o3uh9.11.4`

URL-mode stable latest bootstrap fails before the first metadata fetch when the
local machine is missing a tool required for the later verified install handoff.
Fixture mode stays network-free and does not require host install tools.

The preflight checks:

- `curl`, or the configured downloader command, to fetch release metadata and
  pinned release assets;
- `sha256sum` for checksum sidecar verification;
- `mktemp` for the private bootstrap temp directory;
- `chmod` for installer file permissions;
- root execution or `sudo` for the privileged versioned install handoff.

Failures name `missing tool=<tool>`, explain `needed_for=<purpose>`, and include
a shortest Ubuntu/Debian remediation command. The preflight only checks local
tool availability; it does not download metadata, create install-root staging
directories, or invoke `sudo`.

Regression coverage lives in `scripts/test-stable-latest-bootstrap.py`:

- `test_rejects_missing_downloader_before_network_fetch`
- `test_rejects_missing_checksum_tool_before_network_fetch`
- `test_rejects_missing_downloader_before_guard_url_fetch`
- `test_preflight_rejects_missing_sudo_for_non_root`
- `test_preflight_accepts_root_without_sudo`
- `test_url_mode_fetches_latest_twice_and_emits_no_mutable_latest_url`
