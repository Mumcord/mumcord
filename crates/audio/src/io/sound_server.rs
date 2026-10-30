//! The sound server's sinks and sources on Linux (issue #183).
//!
//! cpal's ALSA host sees sound cards, not the devices the desktop shows: a multi-endpoint USB
//! interface such as the RØDECaster Pro II is one ALSA card but four PipeWire nodes ("System",
//! "Chat", "Microphone", "Main"). So on a PipeWire desktop the list comes from the sound server
//! itself, through its PulseAudio protocol (pipewire-pulse), which reports exactly the sinks and
//! sources the desktop's sound settings show, with their friendly names and the current
//! defaults. Streams then open on the picked node through cpal's PipeWire host, which keeps
//! cpal's real-time callback threads.
//!
//! Only the control thread uses this module; nothing here runs in an audio callback.

use std::ffi::CStr;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use futures::executor::block_on;
use pulseaudio::{Client, protocol};

use super::Direction;
use super::IoError;
use super::devices::ServerNode;

/// How long connecting to the sound server may take before the ALSA host is used instead.
/// The PulseAudio handshake has no socket timeout of its own.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Prefix of cpal's PipeWire device ids, so a listed id opens through cpal unchanged.
pub(crate) const ID_PREFIX: &str = "pipewire:";

/// Property naming the ALSA card id (the `X` in `CARD=X`) of a node backed by a sound card.
const ALSA_CARD_ID: &CStr = c"alsa.id";

/// A connection to the sound server, reopened when the server restarts.
pub(crate) struct SoundServer {
    client: Mutex<Client>,
}

impl SoundServer {
    /// Connects when PipeWire runs with its PulseAudio protocol server; `None` otherwise, and
    /// the caller keeps cpal's ALSA host.
    pub(crate) fn connect() -> Option<Self> {
        if !cpal::available_hosts().contains(&cpal::HostId::PipeWire) {
            return None;
        }
        connect_client()
            .inspect_err(|e| tracing::info!("sound server unavailable, using ALSA: {e}"))
            .ok()
            .map(|client| Self {
                client: Mutex::new(client),
            })
    }

    /// The sinks (output) or sources (input, without the monitors of sinks) and the name of the
    /// current default node.
    pub(crate) fn nodes(
        &self,
        direction: Direction,
    ) -> Result<(Vec<ServerNode>, Option<String>), IoError> {
        let mut client = self.client.lock().unwrap_or_else(|e| e.into_inner());
        match query(&client, direction) {
            Ok(nodes) => Ok(nodes),
            // A restarted sound server drops the connection: reconnect once.
            Err(first) => {
                tracing::debug!("sound server query failed ({first}); reconnecting");
                *client = connect_client()?;
                query(&client, direction)
            }
        }
    }
}

fn connect_client() -> Result<Client, IoError> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Client::from_env(c"Mumcord"));
    });
    rx.recv_timeout(CONNECT_TIMEOUT)
        .map_err(|_| IoError::Backend("sound server connection timed out".into()))?
        .map_err(backend_err)
}

fn query(
    client: &Client,
    direction: Direction,
) -> Result<(Vec<ServerNode>, Option<String>), IoError> {
    let server = block_on(client.server_info()).map_err(backend_err)?;
    let nodes = match direction {
        Direction::Output => block_on(client.list_sinks())
            .map_err(backend_err)?
            .into_iter()
            .map(|s| node(&s.name, s.description.as_deref(), &s.props))
            .collect(),
        Direction::Input => block_on(client.list_sources())
            .map_err(backend_err)?
            .into_iter()
            .filter(|s| s.monitor_of_sink_index.is_none())
            .map(|s| node(&s.name, s.description.as_deref(), &s.props))
            .collect(),
    };
    let default = match direction {
        Direction::Output => server.default_sink_name,
        Direction::Input => server.default_source_name,
    };
    Ok((nodes, default.as_deref().map(text)))
}

fn node(name: &CStr, description: Option<&CStr>, props: &protocol::Props) -> ServerNode {
    ServerNode {
        node_name: text(name),
        description: description.map(text),
        alsa_card: props
            .get_bytes(ALSA_CARD_ID)
            .and_then(|v| CStr::from_bytes_until_nul(v).ok())
            .map(text),
    }
}

fn text(s: &CStr) -> String {
    s.to_string_lossy().into_owned()
}

fn backend_err(err: impl std::fmt::Display) -> IoError {
    IoError::Backend(err.to_string())
}
