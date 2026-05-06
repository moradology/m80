# network-egress

Run one process with outbound egress enabled.

```sh
m80 run --egress outbound -- sh -c '/bin/busybox wget -qO- http://example.com | /bin/busybox head -n 1'
```

Expected stdout begins with the remote document content. The exact line depends
on the selected image's toolchain and the remote site.

The selected profile must contain `sh` and an HTTP client. The minimal release
image ships busybox, so the example invokes busybox applets directly instead of
assuming `wget` and `head` symlinks exist. m80 enables the requested egress
policy; it does not install network tools into the image.

For a no-network run, use:

```sh
m80 run --egress none -- echo isolated
```

Deeper reference: `docs/behaviors/cli/egress-policy.md`.
