# m80 stripped-kernel builder
#
# Builds Linux 6.1.x LTS vmlinux with the m80-stripped.config keep/drop list.
# Pinned to ubuntu:22.04 by digest for toolchain reproducibility.
# Kernel version pin: v6.1.134 (latest 6.1.x LTS as of 2026-05-05).
# Change KERNEL_TAG and re-run to pick up a new 6.1.x point release.

FROM ubuntu@sha256:b359f1067efa76f37863778f7b6d0e8d911e3ee8efa807ad01fbf5dc1ef9006b

ARG KERNEL_TAG=v6.1.134

# Install build toolchain. No interactive prompts (DEBIAN_FRONTEND=noninteractive).
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    bc \
    flex \
    bison \
    libelf-dev \
    libssl-dev \
    libncurses-dev \
    wget \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

# Clone only the tagged commit — shallow clone keeps the layer small.
RUN git clone --depth 1 --branch ${KERNEL_TAG} \
    https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git \
    /linux

WORKDIR /linux

# Determinism pins: strip timestamps and fix embedded build identity strings
# so two builds from the same source + config produce byte-identical vmlinux.
ENV KBUILD_BUILD_TIMESTAMP=0
ENV SOURCE_DATE_EPOCH=0
ENV KBUILD_BUILD_USER=m80
ENV KBUILD_BUILD_HOST=m80-builder

# Copy the stripped kernel config and build script into the image.
COPY m80-stripped.config /config
COPY build.sh /build.sh
RUN chmod +x /build.sh

# Output directory — host binds a volume here to extract vmlinux.
RUN mkdir -p /out

ENTRYPOINT ["/build.sh"]
