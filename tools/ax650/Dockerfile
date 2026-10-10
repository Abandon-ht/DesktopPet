# syntax=docker/dockerfile:1
# Build with --platform linux/arm64. Runtime drivers remain on the board.
FROM ubuntu:22.04@sha256:5ec03bb3441e8b0bf3b4f9cd4629a1ae763010dc3035bb8da3ae6cf026486401
ARG RUST_VERSION=1.95.0
ARG RUST_ARCHIVE_URL=https://static.rust-lang.org/dist/rust-1.95.0-aarch64-unknown-linux-gnu.tar.xz
# Set this when using a separately downloaded official archive/checksum.
ARG RUST_ARCHIVE_SHA256
RUN test "$(dpkg --print-architecture)" = arm64 && \
    test "$RUST_VERSION" = 1.95.0 && \
    apt-get update && apt-get install --yes --no-install-recommends \
      build-essential ca-certificates curl xz-utils python3 pkg-config binutils ripgrep \
      libx11-dev libx11-xcb1 libxcursor1 libxi6 libxrandr2 libxrender1 libxfixes3 \
      libxcb1 libxcb-xkb1 libxkbcommon-dev libxkbcommon-x11-0 \
      libegl1 libgl1 libgles2 libvulkan1 mesa-vulkan-drivers && \
    rm -rf /var/lib/apt/lists/*
RUN mkdir -p /tmp/rust-install && cd /tmp/rust-install && \
    curl --fail --location --retry 3 "$RUST_ARCHIVE_URL" -o rust.tar.xz && \
    if [ -n "$RUST_ARCHIVE_SHA256" ]; then \
      printf '%s  rust.tar.xz\n' "$RUST_ARCHIVE_SHA256" > rust.sha256; \
    else \
      curl --fail --location --retry 3 "$RUST_ARCHIVE_URL.sha256" -o upstream.sha256 && \
      awk 'NR == 1 {print $1 "  rust.tar.xz"}' upstream.sha256 > rust.sha256; \
    fi && \
    sha256sum --check rust.sha256 && \
    tar --extract --xz --file rust.tar.xz && \
    ./rust-1.95.0-aarch64-unknown-linux-gnu/install.sh --prefix=/opt/rust \
      --components=rustc,cargo,rust-std-aarch64-unknown-linux-gnu,rustfmt-preview,clippy-preview && \
    rm -rf /tmp/rust-install
# Packaging records source metadata. Keep this layer after the toolchain so
# existing compiler caches are preserved when adding the packaging dependency.
RUN apt-get update && apt-get install --yes --no-install-recommends git procps && \
    rm -rf /var/lib/apt/lists/*
ENV PATH=/opt/rust/bin:$PATH CARGO_TARGET_DIR=/target CARGO_HOME=/cargo
WORKDIR /workspace
CMD ["bash", "tools/ax650/build.sh"]
