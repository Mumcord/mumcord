//! On a PipeWire desktop every sink and source of the sound server is its own device, named as
//! the desktop names it, and a stream opens on the node that was picked (issue #183).
//!
//! Needs a running PipeWire with its PulseAudio protocol server (pipewire-pulse); skipped when
//! there is none, e.g. on headless CI runners. Only reads the device list and opens streams
//! that stay silent; no setting of the sound server is changed.
#![cfg(target_os = "linux")]

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mumcord_audio::io::{AudioIo, BackendKind, DeviceEvent, DeviceInfo, Direction};

/// Whether both PipeWire sockets exist, so the backend lists the sound server's nodes.
fn sound_server_running() -> bool {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return false;
    };
    runtime.join("pipewire-0").exists() && runtime.join("pulse/native").exists()
}

fn list(direction: Direction) -> Vec<DeviceInfo> {
    AudioIo::list_devices_with(BackendKind::Cpal, direction).expect("device list")
}

#[test]
fn every_sink_and_source_is_listed_by_node_name() {
    if !sound_server_running() {
        eprintln!("skipped: no PipeWire with pipewire-pulse running");
        return;
    }
    for direction in [Direction::Input, Direction::Output] {
        let devices = list(direction);
        let ids: HashSet<&str> = devices.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids.len(), devices.len(), "{direction:?}: ids are unique");
        for device in &devices {
            assert!(device.id.starts_with("pipewire:"), "{direction:?}: node id");
            assert!(!device.name.trim().is_empty(), "{direction:?}: named");
            assert!(
                !device.id.ends_with(".monitor"),
                "{direction:?}: no monitors"
            );
        }
        assert!(
            devices.iter().filter(|d| d.is_default).count() <= 1,
            "{direction:?}: at most one default"
        );
        if let Some(first) = devices.first() {
            assert!(
                first.is_default || devices.iter().all(|d| !d.is_default),
                "{direction:?}: default first"
            );
        }
    }
}

#[test]
fn streams_open_on_the_default_node() {
    if !sound_server_running() {
        eprintln!("skipped: no PipeWire with pipewire-pulse running");
        return;
    }
    let outputs = list(Direction::Output);
    let Some(default) = outputs.iter().find(|d| d.is_default) else {
        eprintln!("skipped: the sound server has no default sink");
        return;
    };
    // Nothing is written to the playback side, so the stream plays silence.
    let handles = AudioIo::start_with(BackendKind::Cpal, None, None).expect("start");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(DeviceEvent::Active {
            direction: Direction::Output,
            device,
            fallback,
        }) = handles.events.recv_timeout(Duration::from_millis(100))
        {
            assert_eq!(device.id, default.id, "the default sink was opened");
            assert!(!fallback);
            return;
        }
    }
    panic!("the output stream did not open");
}
