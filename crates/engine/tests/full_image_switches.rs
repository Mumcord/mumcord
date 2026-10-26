//! The two full-size image switches (FR-1.74) enforced by the engine without a server: with
//! "Offer my full-size images" off no original is kept; with "Load full-size images from
//! others" off nothing is looked up or requested, not even from the cache.

use std::io::Cursor;

use image::{ImageFormat, Rgb, RgbImage};
use mumcord_engine::{Engine, ErrorKind, SecretStore};

fn png() -> Vec<u8> {
    let image = RgbImage::from_fn(8, 8, |x, y| Rgb([x as u8 * 30, y as u8 * 30, 7]));
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("png");
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn switches_stop_keeping_and_loading() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("full_image_switches");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let (engine, _rx) = Engine::new(
        &dir,
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");

    let reference = engine.full_image_keep(png()).await.expect("kept while on");
    assert!(
        engine
            .full_image_fetch(1, &reference, None, None)
            .await
            .is_some(),
        "own image comes from the cache while loading is on"
    );

    let mut settings = engine.settings().expect("settings");
    settings.load_full_images = false;
    engine
        .update_settings(settings.clone())
        .await
        .expect("save");
    assert_eq!(
        engine.full_image_fetch(1, &reference, None, None).await,
        None,
        "no lookup while loading is off"
    );

    settings.offer_full_images = false;
    engine.update_settings(settings).await.expect("save");
    let err = engine.full_image_keep(png()).await.expect_err("refused");
    assert_eq!(err.kind, ErrorKind::InvalidInput);
}
