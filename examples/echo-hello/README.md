# echo-hello

Smallest m80 invocation: no workspace, default egress/profile, one process.

```sh
m80 run -- echo hello
```

Expected stdout:

```text
hello
```

The selected profile must contain `echo`. The minimal release image includes
busybox `echo`.

Deeper reference: `crates/m80-cli/README.md`.
