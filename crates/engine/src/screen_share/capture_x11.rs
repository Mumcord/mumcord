//! Whole-screen capture on X11 (FR-2.42 – FR-2.46).
//!
//! On X11 only the whole screen is shared, so the root window is read with the core `GetImage`
//! request. That needs no extension and no extra crate beyond the `x11rb` the app already uses.
//! The picture is scaled into the encoder's box and converted to I420 in one pass.

use mumcord_screenshare::I420Frame;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};
use x11rb::rust_connection::RustConnection;

use super::capture::CaptureSource;
use super::convert::bgrx_to_i420;
use super::signal::ShareKind;

/// Why the screen cannot be captured.
#[derive(Debug, thiserror::Error)]
pub enum X11CaptureError {
    #[error("cannot connect to the X server: {0}")]
    Connect(String),
    #[error("the screen format is not supported: {0}")]
    Format(String),
}

/// Captures the first screen of the X server named by `DISPLAY`.
pub struct X11Capture {
    conn: RustConnection,
    root: u32,
    width: u16,
    height: u16,
}

impl X11Capture {
    pub fn new() -> Result<Self, X11CaptureError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| X11CaptureError::Connect(e.to_string()))?;
        let setup = conn.setup();
        let screen = &setup.roots[screen_num];
        let depth = screen.root_depth;
        let format = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == depth)
            .ok_or_else(|| {
                X11CaptureError::Format(format!("no pixmap format for depth {depth}"))
            })?;
        // Every X server in practice hands out 32 bits per pixel, little endian, for 24-bit
        // colour; anything else would need its own unpacking.
        if format.bits_per_pixel != 32 || setup.image_byte_order != ImageOrder::LSB_FIRST {
            return Err(X11CaptureError::Format(format!(
                "{} bits per pixel at depth {depth}",
                format.bits_per_pixel
            )));
        }
        Ok(Self {
            root: screen.root,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
            conn,
        })
    }
}

impl CaptureSource for X11Capture {
    /// X11 has no picker: the whole screen is shared.
    fn kind(&self) -> Option<ShareKind> {
        Some(ShareKind::Screen)
    }

    fn frame(&mut self, max_width: u32, max_height: u32) -> Option<I420Frame> {
        let image = self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                self.root,
                0,
                0,
                self.width,
                self.height,
                !0,
            )
            .ok()?
            .reply();
        match image {
            Ok(image) => bgrx_to_i420(
                &image.data,
                u32::from(self.width),
                u32::from(self.height),
                max_width,
                max_height,
            ),
            Err(e) => {
                tracing::warn!("screen capture failed: {e}");
                None
            }
        }
    }
}
