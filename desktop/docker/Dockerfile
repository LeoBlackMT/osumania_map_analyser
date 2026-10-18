# Linux 壳构建环境（与 .github/workflows/shell-build.yml 的 build-linux 同一套依赖）。
# 用法（由 release.ps1 -Linux / -All 调用，也可手写）：
#   docker build -t mma-shell-linux-builder -f desktop/docker/Dockerfile desktop/docker
#   docker run --rm -v "<repo>:/src" -w /src/desktop \
#     -e MMA_FEEDBACK_SIGNING_SEED -e MMA_FEEDBACK_KEY_ID \
#     -e CARGO_TARGET_DIR=/src/desktop/target-linux \
#     mma-shell-linux-builder bash build-linux.sh
FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libxdo-dev \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
