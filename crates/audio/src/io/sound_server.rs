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
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use futures::executor::block_on;
use pulseaudio::{Client, protocol};

use super::Direction;
use super::IoError;
use super::devices::ServerNode;

/// How long connecting to the sound server may take before the ALSA host is used instead.
/// The PulseAudio handshake has no socket timeout of its own.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the sound server may take to answer a device query. Its requests have no timeout
/// either, and a server that stopped answering would hold the control thread forever.
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// Prefix of cpal's PipeWire device ids, so a listed id opens through cpal unchanged.
pub(crate) const ID_PREFIX: &str = "pipewire:";

/// Property naming the ALSA card id (the `X` in `CARD=X`) of a node backed by a sound card.
const ALSA_CARD_ID: &CStr = c"alsa.id";

/// A connection to the sound server, reopened when the server restarts.
pub(crate) struct SoundServer {
    connection: Mutex<Connection>,
}

struct Connection {
    client: Client,
    helper: Helper,
}

/// Runs the sound server's blocking calls on a helper thread, so the caller can give up
/// waiting. A call that timed out leaves its thread behind, blocked in the sound server;
/// until that thread returns no further call is started, so a server that stays hung costs
/// one thread rather than one per poll.
#[derive(Default)]
struct Helper {
    /// Disconnects once the thread of the call that timed out has ended.
    stuck: Option<Receiver<()>>,
}

impl Helper {
    fn run<T: Send + 'static>(
        &mut self,
        what: &'static str,
        timeout: Duration,
        call: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, IoError> {
        if let Some(ended) = &self.stuck {
            if matches!(ended.try_recv(), Err(TryRecvError::Empty)) {
                return Err(IoError::Backend("sound server is not responding".into()));
            }
            tracing::info!("sound server answers again");
            self.stuck = None;
        }
        let (tx, rx) = mpsc::channel();
        let (alive, ended) = mpsc::channel();
        std::thread::spawn(move || {
            let _alive = alive;
            let _ = tx.send(call());
        });
        rx.recv_timeout(timeout).map_err(|_| {
            // Logged here, once per stall: later calls are refused without waiting.
            tracing::warn!("sound server {what} timed out; pausing its queries until it answers");
            self.stuck = Some(ended);
            IoError::Backend(format!("sound server {what} timed out"))
        })
    }

    fn connect(&mut self) -> Result<Client, IoError> {
        self.run("connection", CONNECT_TIMEOUT, || {
            Client::from_env(c"Mumcord")
        })?
        .map_err(backend_err)
    }
}

impl SoundServer {
    /// Connects when PipeWire runs with its PulseAudio protocol server; `None` otherwise, and
    /// the caller keeps cpal's ALSA host.
    pub(crate) fn connect() -> Option<Self> {
        if !cpal::available_hosts().contains(&cpal::HostId::PipeWire) {
            return None;
        }
        let mut helper = Helper::default();
        helper
            .connect()
            .inspect_err(|e| tracing::info!("sound server unavailable, using ALSA: {e}"))
            .ok()
            .map(|client| Self {
                connection: Mutex::new(Connection { client, helper }),
            })
    }

    /// The sinks (output) or sources (input, without the monitors of sinks) and the name of the
    /// current default node.
    pub(crate) fn nodes(
        &self,
        direction: Direction,
    ) -> Result<(Vec<ServerNode>, Option<String>), IoError> {
        let mut connection = self.connection.lock().unwrap_or_else(|e| e.into_inner());
        match connection.query(direction) {
            Ok(nodes) => Ok(nodes),
            // The server stopped answering: a new connection would only hang as well.
            Err(first) if connection.helper.stuck.is_some() => Err(first),
            // A restarted sound server drops the connection: reconnect once.
            Err(first) => {
                tracing::debug!("sound server query failed ({first}); reconnecting");
                connection.client = connection.helper.connect()?;
                connection.query(direction)
            }
        }
    }
}

impl Connection {
    fn query(
        &mut self,
        direction: Direction,
    ) -> Result<(Vec<ServerNode>, Option<String>), IoError> {
        let client = self.client.clone();
        self.helper
            .run("query", QUERY_TIMEOUT, move || request(&client, direction))?
    }
}

fn request(
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A sound server that stays hung used to cost one blocked thread per poll.
    #[test]
    fn a_stalled_call_blocks_further_calls_until_it_returns() {
        let timeout = Duration::from_millis(50);
        let started = Arc::new(AtomicUsize::new(0));
        let (release, stalled) = mpsc::channel::<()>();
        let mut helper = Helper::default();

        let count = started.clone();
        let first = helper.run("query", timeout, move || {
            count.fetch_add(1, Ordering::SeqCst);
            // Returns once the test drops its sender.
            let _ = stalled.recv();
        });
        assert!(first.is_err());

        for _ in 0..3 {
            let count = started.clone();
            let refused = helper.run("query", timeout, move || {
                count.fetch_add(1, Ordering::SeqCst);
            });
            assert!(refused.is_err());
        }
        assert_eq!(started.load(Ordering::SeqCst), 1);

        drop(release);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let answered = loop {
            let result = helper.run("query", timeout, || 7);
            if result.is_ok() || std::time::Instant::now() >= deadline {
                break result;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(matches!(answered, Ok(7)));
    }
}
