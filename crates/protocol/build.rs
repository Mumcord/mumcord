fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use the vendored protoc so builds don't depend on a system install.
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.compile_protos(
        &["proto/Mumble.proto", "proto/MumbleUDP.proto"],
        &["proto"],
    )?;
    Ok(())
}
