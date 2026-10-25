//! Mumble protocol implementation.
//!
//! The control channel ([`connect`]) gives a [`Handle`] for commands and a stream of [`Event`]s.
//! The voice transport plugs in via [`Event::CryptSetup`] / [`Handle::send_crypt_setup`] for UDP
//! crypto and [`Event::UdpTunnel`] / [`Handle::send_tunnel`] for TCP-tunnelled voice.

pub mod client;
pub mod error;
pub mod framing;
pub mod permissions;
mod ratelimit;
pub mod state;
pub mod tls;
pub mod version;
pub mod voice;

pub use client::{
    ConnectConfig, Connection, DisconnectReason, Event, Handle, PLUGIN_DATA_MAX, PluginData,
    TextMessage, TextTarget, connect,
};
pub use error::{
    CommandError, ConnectError, DenyKind, PermissionDenied, ProtocolError, RejectReason,
};
/// Certificate/key DER types used by [`ClientIdentity`] and [`ServerCert`].
pub use rustls::pki_types;
pub use state::{Channel, CryptSetup, ServerInfo, ServerState, User, UserChange};
pub use tls::{CertVerifier, ClientIdentity, ServerCert, cert_hash};
pub use version::MumbleVersion;
pub use voice::{
    CryptStats, LinkStats, MAX_VOICE_PAYLOAD, OutgoingAudio, Transport, VoiceEvent, VoicePacket,
    VoiceTransport,
};

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
