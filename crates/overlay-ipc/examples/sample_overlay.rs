//! Renders a sample overlay panel to a PPM on stdout, for the wizard screenshot. Not shipped.
//! `cargo run -p mumcord-overlay-ipc --features render --example sample_overlay > panel.ppm`

#[cfg(feature = "render")]
fn main() {
    use mumcord_overlay_ipc::render;
    use mumcord_overlay_ipc::wire::{Config, Corner, Person, Scene};

    let config = Config {
        enabled: true,
        corner: Corner::TopLeft,
        size_percent: 100,
        opacity_percent: 100,
        show_whole_channel: true,
        show_channel_name: true,
    };
    let person = |name: &str, talking: bool, muted: bool| Person {
        name: name.into(),
        talking,
        muted,
        deafened: false,
        is_self: false,
    };
    let scene = Scene {
        channel: "Squad voice".into(),
        people: vec![
            person("Mara", true, false),
            person("Dmitri", false, false),
            person("Sven", false, true),
            person("You", true, false),
        ],
    };
    let font = render::font().expect("font");
    let image = render::render(&config, &scene, &font, 2.0).expect("image");
    // A "w h\n" header, then raw premultiplied RGBA; the build script composites it in PIL.
    use std::io::Write;
    let mut out = format!("{} {}\n", image.width, image.height).into_bytes();
    out.extend_from_slice(&image.pixels);
    std::io::stdout().write_all(&out).expect("write");
}

#[cfg(not(feature = "render"))]
fn main() {
    eprintln!("run with --features render");
}
