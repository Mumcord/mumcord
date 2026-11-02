//! Mumble protocol implementation.
//!
//! The control channel ([`connect`]) gives a [`Handle`] for commands and a stream of [`Event`]s.
//! The voice transport plugs in via [`Event::CryptSetup`] / [`Handle::send_crypt_setup`] for UDP
//! crypto and [`Event::UdpTunnel`] / [`Handle::send_tunnel`] for TCP-tunnelled voice.

pub mod client;
pub mod error;
pub mod framing;
pub mod permissions;
pub mod query;
mod ratelimit;
pub mod state;
pub mod tap;
pub mod tls;
pub mod version;
pub mod voice;

pub use client::{
    ConnectConfig, Connection, DisconnectReason, Event, Handle, LowOutcome, PLUGIN_DATA_MAX,
    PluginData, PluginTicket, SyncProgress, TextMessage, TextTarget, UdpPing, connect,
    connect_with_progress, connect_with_tap, set_client_release,
};
pub use error::{
    CommandError, ConnectError, DenyKind, PermissionDenied, ProtocolError, RejectReason,
};
pub use query::{
    LEGACY_ANSWER_LEN, LEGACY_QUERY_LEN, QueryAnswer, decode_answer, encode_legacy_query,
    encode_query,
};
/// Certificate/key DER types used by [`ClientIdentity`] and [`ServerCert`].
pub use rustls::pki_types;
pub use state::{Channel, CryptSetup, ServerInfo, ServerState, User, UserChange};
pub use tap::{PacketTap, TapDirection, TapPacing, TapPluginData, TappedFrame};
pub use tls::{CertVerifier, ClientIdentity, ServerCert, cert_hash};
pub use version::MumbleVersion;
pub use voice::{
    CryptStats, LinkStats, MAX_VOICE_PAYLOAD, OutgoingAudio, Transport, VOICE_TARGET_MAX,
    VoiceEvent, VoicePacket, VoiceTransport, WhisperKind,
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

// Compiles the README example as a doctest so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
