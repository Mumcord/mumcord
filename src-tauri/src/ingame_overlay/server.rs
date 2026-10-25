//! The Unix-socket IPC server (Linux). One accept loop; each connected game gets a thread that
//! reads its [`Hello`], then streams [`Config`] and [`Scene`] and a heartbeat. Writes use a short
//! timeout so a stuck game only drops its own connection. The socket is abstract (see
//! [`mumcord_overlay_ipc::endpoint`]) so games inside the Steam Linux Runtime container reach it;
//! connections from other users are refused.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mumcord_overlay_ipc::wire::{self, Message};

use super::{Shared, key_of};

const HEARTBEAT: Duration = Duration::from_secs(1);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often each client thread checks for a new scene.
const POLL: Duration = Duration::from_millis(100);

/// Owns the listener thread; dropping it stops accepting. The abstract name is released when
/// the listener closes, so there is no socket file to clean up.
pub struct Server {
    running: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Unblock the accept by connecting once.
        let _ = mumcord_overlay_ipc::endpoint::connect();
    }
}

/// Starts the server. Fails with `AddrInUse` while another Mumcord already serves this user.
pub fn start(shared: Arc<Shared>) -> std::io::Result<Server> {
    let listener = mumcord_overlay_ipc::endpoint::bind()?;
    let running = Arc::new(AtomicBool::new(true));
    let server = Server {
        running: running.clone(),
    };
    std::thread::Builder::new()
        .name("mumcord-overlay-ipc".into())
        .spawn(move || accept_loop(&listener, &shared, &running))?;
    Ok(server)
}

fn accept_loop(listener: &UnixListener, shared: &Arc<Shared>, running: &AtomicBool) {
    for stream in listener.incoming() {
        if !running.load(Ordering::Relaxed) {
            return;
        }
        let Ok(stream) = stream else { continue };
        if !mumcord_overlay_ipc::endpoint::same_user(&stream) {
            tracing::warn!("in-game overlay: refused a connection from another user");
            continue;
        }
        let shared = shared.clone();
        // One thread per game; it ends when the game disconnects.
        let _ = std::thread::Builder::new()
            .name("mumcord-overlay-game".into())
            .spawn(move || serve_client(stream, &shared));
    }
}

fn serve_client(mut stream: UnixStream, shared: &Shared) {
    if stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
        || stream.set_read_timeout(Some(WRITE_TIMEOUT)).is_err()
    {
        return;
    }
    let hello = match wire::read_message(&mut stream) {
        Ok(Message::Hello(h)) if h.version == wire::VERSION => h,
        // Unknown version or wrong first message: disable and close.
        Ok(_) | Err(_) => {
            let _ = stream.write_all(&Message::Config(wire::Config::DISABLED).encode());
            return;
        }
    };
    let key = key_of(&hello);
    shared.record_seen(super::SeenGame {
        key: key.clone(),
        exe: hello.exe.clone(),
    });
    tracing::info!(api = ?hello.api, "in-game overlay: a game connected");
    let mut last_config = shared.config_for(&key);
    if stream
        .write_all(&Message::Config(last_config).encode())
        .is_err()
    {
        return;
    }
    tracing::debug!(
        enabled = last_config.enabled,
        "in-game overlay: config sent"
    );
    let mut last_scene = shared.scene_for(&key);
    if last_config.enabled {
        if stream
            .write_all(&Message::Scene(last_scene.clone()).encode())
            .is_err()
        {
            return;
        }
        log_scene(&last_scene);
    }
    let mut last_beat = Instant::now();
    loop {
        std::thread::sleep(POLL);
        let config = shared.config_for(&key);
        if config != last_config {
            last_config = config;
            if stream.write_all(&Message::Config(config).encode()).is_err() {
                return;
            }
            tracing::debug!(enabled = config.enabled, "in-game overlay: config sent");
            // Resend the scene after re-enabling.
            last_scene = wire::Scene::default();
        }
        if config.enabled {
            let scene = shared.scene_for(&key);
            if scene != last_scene {
                last_scene = scene.clone();
                if stream.write_all(&Message::Scene(scene).encode()).is_err() {
                    return;
                }
                log_scene(&last_scene);
            }
        }
        if last_beat.elapsed() >= HEARTBEAT {
            last_beat = Instant::now();
            if stream.write_all(&Message::Heartbeat.encode()).is_err() {
                return;
            }
        }
    }
}

/// Logs what a game was sent, without names.
fn log_scene(scene: &wire::Scene) {
    tracing::debug!(
        people_count = scene.people.len(),
        talking_count = scene.people.iter().filter(|p| p.talking).count(),
        "in-game overlay: scene sent"
    );
}
