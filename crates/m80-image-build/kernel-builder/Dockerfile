# m80 stripped-kernel builder
#
# Builds Linux 6.1.x LTS vmlinux with the m80-stripped.config keep/drop list.
# Pinned to ubuntu:22.04 by digest for toolchain reproducibility.
# Kernel source pin: v6.1.134 peeled commit 420102835862f49e...
# Change KERNEL_COMMIT and re-run to pick up a new 6.1.x point release.

FROM ubuntu@sha256:b359f1067efa76f37863778f7b6d0e8d911e3ee8efa807ad01fbf5dc1ef9006b

ARG UBUNTU_SNAPSHOT=20260505T000000Z
ARG KERNEL_COMMIT=420102835862f49ec15c545594278dc5d2712f42

# Install build toolchain. No interactive prompts (DEBIAN_FRONTEND=noninteractive).
ENV DEBIAN_FRONTEND=noninteractive
RUN printf '%s\n' \
    "deb [check-valid-until=no] https://snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT}/ jammy main restricted universe multiverse" \
    "deb [check-valid-until=no] https://snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT}/ jammy-updates main restricted universe multiverse" \
    "deb [check-valid-until=no] https://snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT}/ jammy-security main restricted universe multiverse" \
    > /etc/apt/sources.list
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

# Fetch only the pinned commit — shallow fetch keeps the layer small without
# trusting a mutable tag name at build time.
RUN git init /linux \
    && git -C /linux remote add origin https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git \
    && git -C /linux fetch --depth 1 origin ${KERNEL_COMMIT} \
    && git -C /linux checkout --detach ${KERNEL_COMMIT}

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
