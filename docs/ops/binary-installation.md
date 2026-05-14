# Binary Installation

This is the deploy-time procedure for host-side binaries that `m80-preflight`
will later verify. It covers only m80's local TCB binaries; guest image
provenance stays in `<rootfs>.manifest.json`.

Install the release binaries as `root:root`, mode `0755` or narrower:

```sh
sudo install -o root -g root -m 0755 firecracker /opt/firecracker/bin/firecracker
sudo install -o root -g root -m 0755 jailer /opt/firecracker/bin/jailer
sudo install -o root -g root -m 0755 m80 /opt/m80/bin/m80
sudo install -o root -g root -m 0755 m80-cli /opt/m80/bin/m80-cli
sudo install -o root -g root -m 0755 m80-jailer-harden /opt/m80/bin/m80-jailer-harden
sudo install -o root -g root -m 0755 m80-net-helper /opt/m80/bin/m80-net-helper
sudo install -o root -g root -m 0644 firecracker-seccomp-filter.bin /opt/firecracker/bin/firecracker-seccomp-filter.bin
```

Write `/opt/m80/artifacts/host-binaries.manifest.json` from the exact installed
binary bytes. The seccomp filter is validated as launch material by path and
file identity during preflight; it is not a host-binary manifest entry. The
manifest schema is:

```json
{
  "binaries": [
    {
      "name": "firecracker",
      "path": "/opt/firecracker/bin/firecracker",
      "sha256": "<64 lowercase hex chars>"
    },
    {
      "name": "jailer",
      "path": "/opt/firecracker/bin/jailer",
      "sha256": "<64 lowercase hex chars>"
    },
    {
      "name": "m80",
      "path": "/opt/m80/bin/m80",
      "sha256": "<64 lowercase hex chars>"
    },
    {
      "name": "m80_cli",
      "path": "/opt/m80/bin/m80-cli",
      "sha256": "<64 lowercase hex chars>"
    },
    {
      "name": "m80_jailer_harden",
      "path": "/opt/m80/bin/m80-jailer-harden",
      "sha256": "<64 lowercase hex chars>"
    },
    {
      "name": "m80_net_helper",
      "path": "/opt/m80/bin/m80-net-helper",
      "sha256": "<64 lowercase hex chars>"
    }
  ],
  "schema_version": 2
}
```

Use `sha256sum` or an equivalent structured release tool to populate the hash
fields. Do not hand-edit the digest after copying a replacement binary; replace
the binary and regenerate the manifest from the installed bytes.

`m80-preflight` checks the configured Firecracker, jailer, hardening-wrapper,
and network-helper paths against this manifest. It opens every manifest path with `O_NOFOLLOW`,
hashes the opened file descriptor, and rejects non-root-owned or writable
binaries. A version string alone is not accepted as binary identity.
