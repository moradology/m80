# workspace-roundtrip

Expose a host directory at `/workspace`, write a file in the guest, and extract
changes back only when the process exits successfully.

```sh
tmp="$(mktemp -d)"
m80 run --workspace "$tmp" --cwd /workspace --writeback on-success -- sh -c 'echo done > result.txt'
cat "$tmp/result.txt"
```

Expected stdout from the final `cat`:

```text
done
```

The selected profile must contain `sh`, `echo`, and `cat`. The minimal release
image includes busybox versions.

Deeper reference: `docs/behaviors/cli/workspace-visibility.md` and
`docs/behaviors/cli/writeback-policy.md`.
