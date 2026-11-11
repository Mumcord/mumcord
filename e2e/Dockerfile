# E2E image: builds and drives Mumcord headless (Xvfb) through tauri-driver + WebKitWebDriver.
# openbox: a window manager for the overlay's on-top check; lavapipe (mesa-vulkan-drivers): a
# software Vulkan driver for the in-game overlay check (AC-1.33, AC-1.34b).
# pipewire, wireplumber and pipewire-bin (pw-cat): the share-sound specs (AC-7.61 to 7.66) need a real
# PipeWire graph with a null sink; pulseaudio is the daemon of the PulseAudio-only variant (AC-7.66).
# libglib2.0-bin (gdbus): the tray and camera-portal specs talk to the session bus with it.
# Ubuntu because Arch's webkit2gtk ships without WebKitWebDriver.
#
# Stages (#562): `runtime` has only what the test shards run (no compiler, headers or Rust), so
# CI pulls a smaller image there; `build` (the default) adds the toolchain for compiling.

FROM ubuntu:26.04@sha256:6a06111c3b0a8600feadb270aec771dc239bf407b25625f93f44ab00e11f4d3c AS base

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_MAJOR=24

# Ubuntu mirrors and download.docker.com fail now and then (e.g. "Mirror sync in progress?");
# apt and curl retry with a growing delay instead of failing the image build.
RUN echo 'Acquire::Retries "5";' > /etc/apt/apt.conf.d/80-retries

# The libraries the app links at runtime, plus the test environment.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl gnupg git file \
      libwebkit2gtk-4.1-0 libayatana-appindicator3-1 librsvg2-2 libxdo3 libasound2t64 \
      libssl3t64 webkit2gtk-driver libvpx9 libpipewire-0.3-0t64 \
      xvfb xauth dbus dbus-x11 libnss-wrapper gnome-keyring libsecret-tools xdotool \
      sqlite3 openssl \
      openbox libvulkan1 mesa-vulkan-drivers \
      pipewire pipewire-bin wireplumber pulseaudio libglib2.0-bin \
    && rm -rf /var/lib/apt/lists/*

# Docker CLI + compose plugin: helpers run testenv/up.sh and reset.sh through the host's socket.
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL --retry 5 --retry-all-errors https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc \
    && echo "deb [signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu noble stable" \
       > /etc/apt/sources.list.d/docker.list \
    && curl -fsSL --retry 5 --retry-all-errors https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
       | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" \
       > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update && apt-get install -y --no-install-recommends \
       docker-ce-cli docker-compose-plugin nodejs \
    && rm -rf /var/lib/apt/lists/*

COPY container-run.sh /usr/local/bin/mumcord-e2e
ENTRYPOINT ["/usr/local/bin/mumcord-e2e"]

# The compile toolchain: headers, build tools and Rust in /opt, world-readable; the container
# runs as the host user with its own CARGO_HOME cache (mounted), so registry downloads survive.
FROM base AS toolchain

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config cmake nasm meson ninja-build \
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libasound2-dev \
      libssl-dev libvpx-dev libpipewire-0.3-dev libclang-dev \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup PATH=/opt/cargo/bin:$PATH
# rustup-init is pinned by version and SHA-256 (x86_64 only, like the rest of this image).
ARG RUSTUP_VERSION=1.29.3
ARG RUSTUP_INIT_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
RUN curl -fsSL -o /tmp/rustup-init "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
    && echo "${RUSTUP_INIT_SHA256}  /tmp/rustup-init" | sha256sum -c - \
    && chmod +x /tmp/rustup-init \
    && CARGO_HOME=/opt/cargo /tmp/rustup-init -y --profile minimal --default-toolchain stable \
    && rm /tmp/rustup-init \
    && chmod -R a+rX /opt/rustup /opt/cargo

FROM toolchain AS driver

ARG TAURI_DRIVER_VERSION=2.0.6
RUN CARGO_HOME=/opt/cargo cargo install --locked tauri-driver --version ${TAURI_DRIVER_VERSION}

# What the test shards run.
FROM base AS runtime
COPY --from=driver /opt/cargo/bin/tauri-driver /usr/local/bin/tauri-driver

# The default: compiling and testing in one image (local runs and the CI build job).
FROM toolchain AS build
COPY --from=driver /opt/cargo/bin/tauri-driver /opt/cargo/bin/tauri-driver
