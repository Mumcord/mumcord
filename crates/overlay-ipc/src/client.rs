//! The game side of the channel. Runs on its own background thread inside the game, never on
//! the game's render thread: it connects, reads messages with blocking I/O and hands every change
//! to a callback, which publishes the result for the render thread to pick up without waiting.

use std::io::Write;
use std::time::Duration;

use crate::wire::{self, Config, Hello, Message, Scene};

/// Mumcord sends a heartbeat every second; three missed ones make the data stale.
pub const STALE_AFTER: Duration = Duration::from_secs(3);
/// Pause between connection attempts while Mumcord is not running.
pub const RETRY_EVERY: Duration = Duration::from_secs(2);

/// What the game should draw: `None` (nothing) or the options and the current scene.
pub type Update<'a> = Option<(&'a Config, &'a Scene)>;

/// Applies one incoming message to the client state. Returns whether the drawable state changed.
pub fn apply(message: Message, config: &mut Config, scene: &mut Scene) -> bool {
    match message {
        Message::Config(c) => {
            let changed = *config != c;
            *config = c;
            changed
        }
        Message::Scene(s) => {
            let changed = *scene != s;
            *scene = s;
            changed && config.enabled
        }
        Message::Heartbeat | Message::Hello(_) => false,
    }
}

/// Connects to Mumcord forever, calling `on_update` whenever what to draw changes; `None` while
/// not connected, disabled or stale. Never returns.
pub fn run(hello: &Hello, mut on_update: impl FnMut(Update<'_>)) -> ! {
    loop {
        if let Ok(stream) = connect() {
            serve(stream, hello, &mut on_update);
            on_update(None);
        }
        std::thread::sleep(RETRY_EVERY);
    }
}

fn serve(mut stream: Stream, hello: &Hello, on_update: &mut impl FnMut(Update<'_>)) {
    if stream
        .write_all(&Message::Hello(hello.clone()).encode())
        .is_err()
    {
        return;
    }
    let mut config = Config::DISABLED;
    let mut scene = Scene::default();
    // A read error is a closed, broken or silent (stale) connection; all end the session.
    while let Ok(message) = wire::read_message(&mut stream) {
        if apply(message, &mut config, &mut scene) {
            on_update(config.enabled.then_some((&config, &scene)));
        }
    }
}

#[cfg(target_os = "linux")]
type Stream = std::os::unix::net::UnixStream;

#[cfg(target_os = "linux")]
fn connect() -> std::io::Result<Stream> {
    let stream = crate::endpoint::connect()?;
    stream.set_read_timeout(Some(STALE_AFTER))?;
    stream.set_write_timeout(Some(STALE_AFTER))?;
    Ok(stream)
}

/// A named pipe opened as a file. There is no read timeout on a synchronous pipe handle; a hung
/// Mumcord would keep the old scene, so Mumcord closes the pipe when it quits, which ends the read.
#[cfg(windows)]
type Stream = std::fs::File;

#[cfg(windows)]
fn connect() -> std::io::Result<Stream> {
    let path = crate::endpoint::path()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
}

#[cfg(not(any(target_os = "linux", windows)))]
type Stream = std::io::Empty;

#[cfg(not(any(target_os = "linux", windows)))]
fn connect() -> std::io::Result<Stream> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Corner, Person};

    #[test]
    fn scenes_only_count_while_enabled() {
        let mut config = Config::DISABLED;
        let mut scene = Scene::default();
        let s = Scene {
            channel: String::new(),
            people: vec![Person {
                name: "A".into(),
                talking: true,
                muted: false,
                deafened: false,
                is_self: false,
            }],
        };
        assert!(!apply(Message::Scene(s.clone()), &mut config, &mut scene));
        assert_eq!(scene, s);
        let enabled = Config {
            enabled: true,
            corner: Corner::TopRight,
            ..Config::DISABLED
        };
        assert!(apply(Message::Config(enabled), &mut config, &mut scene));
        assert!(!apply(Message::Config(enabled), &mut config, &mut scene));
        assert!(!apply(Message::Heartbeat, &mut config, &mut scene));
        assert!(!apply(Message::Scene(s), &mut config, &mut scene));
        assert!(apply(
            Message::Scene(Scene::default()),
            &mut config,
            &mut scene
        ));
    }
}
