# E2E image: builds and drives Mumcord headless (Xvfb) through tauri-driver + WebKitWebDriver.
# openbox: a window manager for the overlay's on-top check; lavapipe (mesa-vulkan-drivers): a
# software Vulkan driver for the in-game overlay check (AC-1.33, AC-1.34b).
# Ubuntu because Arch's webkit2gtk ships without WebKitWebDriver.
FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_MAJOR=24
ARG TAURI_DRIVER_VERSION=2.0.6

# Ubuntu mirrors and download.docker.com fail now and then (e.g. "Mirror sync in progress?");
# apt and curl retry with a growing delay instead of failing the image build.
RUN echo 'Acquire::Retries "5";' > /etc/apt/apt.conf.d/80-retries

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl gnupg git build-essential pkg-config cmake file \
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libasound2-dev \
      libssl-dev webkit2gtk-driver libvpx-dev libpipewire-0.3-dev libclang-dev \
      xvfb xauth dbus dbus-x11 libnss-wrapper gnome-keyring libsecret-tools xdotool \
      sqlite3 openssl \
      openbox libvulkan1 mesa-vulkan-drivers \
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

# Rust toolchain world-readable in /opt; the container runs as the host user with its own
# CARGO_HOME cache (mounted), so registry downloads survive between runs.
ENV RUSTUP_HOME=/opt/rustup PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | CARGO_HOME=/opt/cargo sh -s -- -y --profile minimal --default-toolchain stable \
    && CARGO_HOME=/opt/cargo cargo install --locked tauri-driver --version ${TAURI_DRIVER_VERSION} \
    && chmod -R a+rX /opt/rustup /opt/cargo

COPY container-run.sh /usr/local/bin/mumcord-e2e
ENTRYPOINT ["/usr/local/bin/mumcord-e2e"]
