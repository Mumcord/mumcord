//! The platform-fed audio backend inside the engine, without a server (Epic 5, FR-5.25): the
//! app supplies the audio, the system cancels echo, there is one logical device per direction,
//! and the microphone test runs the whole way from the app's microphone callback through the
//! processing chain back to its speaker callback.

mod common;

use std::time::Duration;

use common::host::{Host, SAMPLE_RATE, rms, speech_like};
use mumcord_audio::io::EXTERNAL_DEVICE_ID;
use mumcord_engine::{EchoCancellationMode, Engine, SecretStore};

#[tokio::test(flavor = "multi_thread")]
async fn the_microphone_test_runs_through_the_apps_audio() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (host, audio) = Host::start();
    let (engine, _events) = Engine::with_external_audio(
        dir.path(),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
        audio,
    )
    .expect("engine");

    assert_eq!(
        engine.echo_cancellation_mode(),
        EchoCancellationMode::System
    );
    let settings = engine.settings().expect("settings");
    assert!(settings.noise_suppression && settings.auto_gain);

    // One logical device per direction, before and after the audio starts.
    let idle = engine.audio_devices().await.expect("idle devices");
    engine.mic_test_start(None).await.expect("test start");
    let running = engine.audio_devices().await.expect("running devices");
    for list in [&idle, &running] {
        assert_eq!(list.inputs.len(), 1);
        assert_eq!(list.outputs.len(), 1);
        assert_eq!(list.inputs[0].id, EXTERNAL_DEVICE_ID);
        assert_eq!(list.outputs[0].id, EXTERNAL_DEVICE_ID);
    }

    // Record 1.5 s of speech; nothing is played meanwhile.
    let recording_from = host.played_len();
    host.speak(&speech_like(SAMPLE_RATE * 3 / 2, 0.5));
    tokio::time::sleep(Duration::from_millis(1700)).await;
    let playing_from = host.played_len();
    assert!(
        host.played_from(recording_from).iter().all(|&s| s == 0.0),
        "sound was played while recording"
    );

    // Play it back: the recorded speech comes out of the speaker callback.
    engine.mic_test_stop_recording();
    tokio::time::sleep(Duration::from_millis(2000)).await;
    let played = host.played_from(playing_from);
    let level = rms(&played[..SAMPLE_RATE * 3 / 2]);
    assert!(
        level > 0.02,
        "the recording was not played back (level {level})"
    );
    engine.mic_test_stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn device_audio_keeps_the_engines_echo_canceller() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (engine, _events) = Engine::new(
        dir.path(),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    assert_eq!(
        engine.echo_cancellation_mode(),
        EchoCancellationMode::Engine
    );
}
