//! Icons and screenshots the plugin list points to. The UI never fetches from the network, so
//! the app loads them here and hands them over as bytes with a known image type.

use reqwest::Url;

use super::{Client, RegistryError};

/// Largest icon the store shows.
pub const MAX_ICON_BYTES: usize = 512 * 1024;
/// Largest screenshot the store shows.
pub const MAX_SCREENSHOT_BYTES: usize = 2 * 1024 * 1024;

/// A downloaded raster image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub bytes: Vec<u8>,
    /// `image/png`, `image/jpeg`, `image/webp` or `image/gif`.
    pub mime: &'static str,
}

/// The image type from the file's first bytes. The server's Content-Type is not trusted, and
/// SVG is refused because it is a document that can carry scripts and external references.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

impl Client {
    /// Downloads an image of at most `max` bytes under the client's URL policy.
    pub async fn fetch_image(&self, url: &str, max: usize) -> Result<Image, RegistryError> {
        let url = Url::parse(url).map_err(|_| RegistryError::InvalidUrl)?;
        let bytes = self.get_body(&url, max).await?;
        let mime = sniff_image(&bytes)
            .ok_or_else(|| RegistryError::Format("not a supported image".to_owned()))?;
        Ok(Image { bytes, mime })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_raster_formats() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_image(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"GIF89a..."), Some("image/gif"));
    }

    #[test]
    fn refuses_svg_html_and_short_input() {
        assert_eq!(
            sniff_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            None
        );
        assert_eq!(sniff_image(b"<?xml version=\"1.0\"?><svg/>"), None);
        assert_eq!(sniff_image(b"<html>"), None);
        assert_eq!(sniff_image(b"RIFF"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[tokio::test]
    async fn fetches_images_within_the_cap_and_refuses_the_rest() {
        let server = super::super::test_server::TestServer::start();
        server
            .files
            .put("/icon.png", b"\x89PNG\r\n\x1a\nbody".to_vec());
        server.files.put("/big.png", vec![0x89; 64]);
        server.files.put("/logo.svg", b"<svg/>".to_vec());
        let client = Client::new(super::super::UrlPolicy::new(true)).expect("test step");
        let url = |path: &str| format!("{}{path}", server.base);

        let icon = client
            .fetch_image(&url("icon.png"), MAX_ICON_BYTES)
            .await
            .expect("test step");
        assert_eq!(icon.mime, "image/png");
        assert!(matches!(
            client.fetch_image(&url("big.png"), 16).await,
            Err(RegistryError::TooLarge(16))
        ));
        assert!(matches!(
            client.fetch_image(&url("logo.svg"), MAX_ICON_BYTES).await,
            Err(RegistryError::Format(_))
        ));
        let insecure = Client::new(super::super::UrlPolicy::new(false)).expect("test step");
        assert!(
            insecure
                .fetch_image(&url("icon.png"), MAX_ICON_BYTES)
                .await
                .is_err()
        );
    }
}
