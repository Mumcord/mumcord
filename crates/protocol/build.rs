fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use the vendored protoc so builds don't depend on a system install. The vendored binaries
    // only exist for the platforms protoc-bin-vendored ships; elsewhere (FreeBSD) prost-build
    // falls back to PROTOC or a `protoc` from the system, which the build then needs installed.
    let mut config = prost_build::Config::new();
    if let Ok(protoc) = protoc_bin_vendored::protoc_bin_path() {
        config.protoc_executable(protoc);
    }
    config.compile_protos(&["proto/Mumble.proto", "proto/MumbleUDP.proto"], &["proto"])?;
    Ok(())
}
