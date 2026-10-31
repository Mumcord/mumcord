//! The microphone is opened once: listing devices (the wizard's microphone step and the audio
//! settings do that on every visit) opens no stream, and concurrent first uses share one. Each
//! open can show the macOS microphone prompt again for a build without a stable signature.

use mumcord_audio::io::{BACKEND_ENV, input_opens};
use mumcord_engine::{Engine, SecretStore};

#[tokio::test(flavor = "multi_thread")]
async fn microphone_is_opened_once() {
    // SAFETY: set before the engine starts any thread; this binary holds only this test.
    unsafe {
        std::env::set_var(BACKEND_ENV, "null");
    }
    let dir = tempfile::tempdir().expect("temp dir");
    let (engine, _events) = Engine::new(
        dir.path(),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");

    for _ in 0..3 {
        let devices = engine.audio_devices().await.expect("devices");
        assert_eq!(devices.inputs.len(), 1);
    }
    assert_eq!(input_opens(), 0, "listing devices opened the microphone");

    let (first, second) = tokio::join!(engine.mic_test_start(None), engine.mic_test_start(None));
    first.expect("first start");
    second.expect("second start");
    engine
        .audio_devices()
        .await
        .expect("devices during the test");
    assert_eq!(input_opens(), 1);

    engine.mic_test_stop();
}
