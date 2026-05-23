# Binary Installation

This is the deploy-time procedure for host-side binaries that `m80-preflight`
will later verify. It covers only m80's local TCB binaries; guest image
provenance stays in `<rootfs>.manifest.json`.

## Normal Linux Install

Use the release installer for normal Linux hosts:

`curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh`

The installer stages the matched m80 binary, host helpers, guest artifacts,
manifest material, default profile, and active version pointer. After it
finishes, `m80 run -- echo hello` is the public smoke command.

Manual binary and artifact placement is an advanced/operator path. Use it only
for release engineering, forensics, or hosts where the release installer cannot
own final placement; keep every binary, guest artifact, and manifest from the
same release set.

## Advanced Manual Placement

Install the release binaries as `root:root`, mode `0755` or narrower:

```sh
sudo install -o root -g root -m 0755 firecracker /opt/firecracker/bin/firecracker
sudo install -o root -g root -m 0755 jailer /opt/firecracker/bin/jailer
sudo install -o root -g root -m 0755 m80 /opt/m80/bin/m80
sudo install -o root -g root -m 0755 m80-jailer-harden /opt/m80/bin/m80-jailer-harden
sudo install -o root -g root -m 0755 m80-net-helper /opt/m80/bin/m80-net-helper
sudo install -o root -g root -m 0644 firecracker-seccomp-filter.bin /opt/firecracker/bin/firecracker-seccomp-filter.bin
```

Write `/opt/m80/artifacts/host-binaries.manifest.json` from the exact installed
binary and launch-material bytes. The seccomp filter is recorded as launch
material, not as an executable host binary. The manifest schema is:

```json
{
  "binaries": [
    {
      "name": "firecracker",
      "path": "/opt/firecracker/bin/firecracker",
      "sha256": "<64 lowercase hex chars>",
      "version": "v1.15.1"
    },
    {
      "name": "jailer",
      "path": "/opt/firecracker/bin/jailer",
      "sha256": "<64 lowercase hex chars>",
      "version": "v1.15.1"
    },
    {
      "name": "m80",
      "path": "/opt/m80/bin/m80",
      "sha256": "<64 lowercase hex chars>",
      "version": "m80 0.0.0"
    },
    {
      "name": "m80_jailer_harden",
      "path": "/opt/m80/bin/m80-jailer-harden",
      "sha256": "<64 lowercase hex chars>",
      "version": "m80-jailer-harden 0.0.0"
    },
    {
      "name": "m80_net_helper",
      "path": "/opt/m80/bin/m80-net-helper",
      "sha256": "<64 lowercase hex chars>",
      "version": "m80-net-helper 0.0.0"
    }
  ],
  "launch_material": [
    {
      "name": "firecracker_seccomp_filter",
      "path": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
      "sha256": "<64 lowercase hex chars>",
      "version": "v1.15.1"
    }
  ],
  "schema_version": 4
}
```

Use `m80-preflight`'s host-binaries manifest generator, `sha256sum`, or an
equivalent structured release tool to populate the hash and version fields. Do
not hand-edit the digest after copying a replacement binary; replace the binary
and regenerate the manifest from the installed bytes.

`m80-preflight` checks the configured Firecracker, jailer, hardening-wrapper,
network-helper, and Firecracker seccomp-filter paths against this manifest. It
opens every manifest path with `O_NOFOLLOW`, hashes the opened file
descriptor, and rejects non-root-owned or writable binaries and launch
material. A version string alone is not accepted as binary identity.
