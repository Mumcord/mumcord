#!/bin/sh
# Builds mumpeer and mumbot inside the jail and puts both binaries in /root/bin.
#
# mumbot depends on mumcord-protocol, whose build script asks protoc-bin-vendored for a protoc
# binary; that crate ships none for FreeBSD and fails with "Error { os: \"freebsd\", arch: ... }".
# The fix is a path patch to a copy of the crate whose build script uses the system protoc.
set -e

RELAY_SRC=/root/src/mumpeer
BOT_SRC=/root/src/mumbot

# Nothing about the versions is pinned here: the jail is a test rig and should track main.
[ -d "$RELAY_SRC" ] || git clone --depth 1 https://github.com/Mumcord/mumpeer.git "$RELAY_SRC"
[ -d "$BOT_SRC" ] || git clone --depth 1 https://github.com/Mumcord/mumbot.git "$BOT_SRC"

# mumpeer builds as is.
cd "$RELAY_SRC"
cargo build --release
install -m 755 target/release/mumpeer /root/bin/mumpeer

# Fetch mumbot's crates, then patch the protocol crate that the registry copy ships.
# (Not --locked: the path patch below has to be allowed to update the lock file.)
cd "$BOT_SRC"
cargo fetch

# The crate itself, not the whole registry: the extracted sources only appear at build time, so
# unpack the registry cache, and fall back to the download URL when that cache is empty.
VERSION=$(awk '/^name = "mumcord-protocol"/{found = 1} found && /^version/{gsub(/"/, "", $3); print $3; exit}' Cargo.lock)
[ -n "$VERSION" ] || { echo "build-stack: mumcord-protocol has no version in Cargo.lock" >&2; exit 1; }
CRATE=$(find "$HOME/.cargo/registry/cache" -maxdepth 2 -name "mumcord-protocol-$VERSION.crate" | head -1)
rm -rf /root/mumcord-protocol
mkdir -p /root/mumcord-protocol
if [ -n "$CRATE" ]; then
    tar xzf "$CRATE" -C /root/mumcord-protocol --strip-components=1
else
    curl -fsSL "https://static.crates.io/crates/mumcord-protocol/mumcord-protocol-$VERSION.crate" |
        tar xz -C /root/mumcord-protocol --strip-components=1
fi
cat > /root/mumcord-protocol/build.rs <<'RUST'
use std::path::{Path, PathBuf};

fn protoc_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // FreeBSD has no vendored protoc binary, so use the system one.
    if let Ok(path) = std::env::var("PROTOC") {
        return Ok(PathBuf::from(path));
    }
    for candidate in ["/usr/local/bin/protoc", "/usr/bin/protoc"] {
        if Path::new(candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }
    Ok(protoc_bin_vendored::protoc_bin_path()?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_path()?);
    config.compile_protos(&["proto/Mumble.proto", "proto/MumbleUDP.proto"], &["proto"])?;
    Ok(())
}
RUST
grep -q '^\[patch.crates-io\]' Cargo.toml ||
    printf '\n[patch.crates-io]\nmumcord-protocol = { path = "/root/mumcord-protocol" }\n' >> Cargo.toml
cargo build --release
install -m 755 target/release/mumbot /root/bin/mumbot
