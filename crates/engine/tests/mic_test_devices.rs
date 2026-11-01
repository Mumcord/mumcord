//! A microphone test can run on devices the caller has not saved (#149: the wizard rerun must
//! not write settings before Finish). The stored devices stay untouched, so a cancelled wizard
//! leaves the engine on them.

use std::time::Duration;

use mumcord_audio::io::{BACKEND_ENV, input_opens};
use mumcord_engine::{Engine, MicTestDevices, SecretStore};

/// The audio control thread switches devices in the background; give it a moment.
async fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mic_test_on_unsaved_devices_saves_nothing() {
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

    let saved = engine.settings().expect("settings");
    assert_eq!(saved.input_device, None);

    // The baseline is taken before the start: the start opens the microphone once for the audio
    // engine and once more for the switch, and a fast machine finishes the switch before
    // `mic_test_start` returns, so a baseline read afterwards could already include it.
    let opens = input_opens();
    engine
        .mic_test_start(Some(MicTestDevices {
            input: Some("unsaved-microphone".into()),
            output: Some("unsaved-speakers".into()),
        }))
        .await
        .expect("test start");

    let switched = wait_until(|| input_opens() >= opens + 2).await;
    assert!(switched, "the test must open the microphone again");

    let during = engine.settings().expect("settings");
    assert_eq!(during.input_device, None, "the test saved the microphone");
    assert_eq!(during.output_device, None, "the test saved the speakers");

    engine.mic_test_stop();
    let after = engine.settings().expect("settings");
    assert_eq!(after.input_device, None);
    assert_eq!(after.output_device, None);
}
