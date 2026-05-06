#!/bin/sh
set -eu

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

m80 run --workspace "$tmp" --cwd /workspace --writeback on-success -- sh -c 'echo done > result.txt'
cat "$tmp/result.txt"
