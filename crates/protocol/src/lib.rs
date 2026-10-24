//! Mumble protocol implementation.

/// Control-channel messages generated from the vendored `Mumble.proto`.
#[allow(clippy::all, missing_docs)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/mumble_proto.rs"));
}

/// UDP voice messages generated from the vendored `MumbleUDP.proto`.
#[allow(clippy::all, missing_docs)]
pub mod udp_proto {
    include!(concat!(env!("OUT_DIR"), "/mumble_udp.rs"));
}
