#!/bin/bash
# m80 stripped-kernel build script.
#
# Runs inside the m80-kernel-builder container. Copies the config, runs
# olddefconfig to fill any new symbols with their defaults, builds vmlinux,
# prints the config sha256 (used as the output filename component on the host),
# and copies vmlinux to /out.
#
# Host invocation (from workspace root):
#   docker build -t m80-kernel-builder crates/m80-image-build/kernel-builder/
#   docker run --rm \
#     -v "$(pwd)/crates/m80-image-build/kernels:/out" \
#     m80-kernel-builder

set -euo pipefail

cd /linux

# Copy the committed config into the kernel tree.
cp /config .config

# Fill any new Kconfig symbols with their safe defaults. This is idempotent:
# running it on an already-complete config is a no-op.
make olddefconfig

# Compute the sha256 of the resolved .config (after olddefconfig so the hash
# reflects the actual build inputs, not the seed config).
CONFIG_SHA=$(sha256sum .config | awk '{print $1}')
echo "config-sha256: ${CONFIG_SHA}"

# Build vmlinux only. No modules (CONFIG_MODULES=n in our config), no bzImage
# (Firecracker loads vmlinux directly). Use all available CPUs.
make vmlinux -j"$(nproc)"

# Copy to output directory with the config-sha filename.
cp vmlinux "/out/vmlinux-m80-${CONFIG_SHA}.bin"
echo "output: /out/vmlinux-m80-${CONFIG_SHA}.bin"
