# Public Install Invocation

Behavior bead: `m80-o3uh9.14.5`.

The public `install.sh` path downloads release metadata, verifies the selected
bundle, extracts `bin/m80` from that bundle, verifies the extracted binary
identity, and then invokes that exact extracted path:

```text
<temporary-extract-dir>/bin/m80 install --bundle-url <verified-bundle-url> ...
```

The handoff does not resolve `m80` through ambient `PATH`. A checkout, old
system install, or other executable earlier in `PATH` cannot replace the
release binary selected by the verified bundle.

## Child Environment

The extracted `m80 install` runs under an explicit `env -i` allowlist. The
public installer does not pass through ambient `M80_*` variables, including
developer, fixture, profile, artifact, config, run-root, install-root, release,
and verifier overrides. Public install selection comes from the verified
release selector and the installer arguments, not from the caller's shell
environment.

Allowed child variables are limited to process basics and network trust/proxy
settings:

- `HOME`
- `PATH=/usr/sbin:/usr/bin:/sbin:/bin`
- `TMPDIR=/tmp`
- `LANG`, `LC_ALL`, and `LC_CTYPE`
- `SSL_CERT_FILE`, `SSL_CERT_DIR`, `CURL_CA_BUNDLE`, and
  `REQUESTS_CA_BUNDLE`
- `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, `http_proxy`, `https_proxy`, and
  `no_proxy`

Install-root selection stays explicit: pass `--install-root PATH` to the
installer. Runtime operator overrides such as `M80_KERNEL_IMAGE`,
`M80_ROOTFS_IMAGE`, `M80_CONFIG`, `M80_PROFILE`, and `M80_RUN_ROOT` are for
direct `m80` commands, not for the public release install handoff.
