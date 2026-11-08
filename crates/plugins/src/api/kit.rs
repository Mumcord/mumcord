//! The UI kit Mumcord serves to pages of API 1.2 plugins (FR-13.18, FR-13.19): a stylesheet, the
//! app's two font faces and their licence, compiled into the binary from `ui/src/plugin-kit/` so
//! nothing is ever read from disk or fetched from the network.

use crate::permissions::{API_1_2, is_api_1_2};

/// The kit's folder below the reserved segment for API major 1 (FR-13.19); a future 2.0 kit would
/// live at `v2/` next to it.
const V1: &str = "v1/";

/// One kit file: its bytes and MIME type (FR-13.23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KitFile {
    pub body: &'static [u8],
    pub mime: &'static str,
}

/// Every file of the v1 kit, by its path below `_mumcord/v1/`.
const FILES: [(&str, KitFile); 4] = [
    (
        "kit.css",
        KitFile {
            body: include_bytes!("../../../../ui/src/plugin-kit/kit.css"),
            mime: "text/css; charset=utf-8",
        },
    ),
    (
        "fonts/instrument-sans-latin-wght-normal.woff2",
        KitFile {
            body: include_bytes!(
                "../../../../ui/src/plugin-kit/fonts/instrument-sans-latin-wght-normal.woff2"
            ),
            mime: "font/woff2",
        },
    ),
    (
        "fonts/bricolage-grotesque-latin-wght-normal.woff2",
        KitFile {
            body: include_bytes!(
                "../../../../ui/src/plugin-kit/fonts/bricolage-grotesque-latin-wght-normal.woff2"
            ),
            mime: "font/woff2",
        },
    ),
    (
        "fonts/OFL.txt",
        KitFile {
            body: include_bytes!("../../../../ui/src/plugin-kit/fonts/OFL.txt"),
            mime: "text/plain; charset=utf-8",
        },
    ),
];

/// The kit file for `path` (relative to `web/`, already known to be under the reserved segment)
/// for a plugin built against `api`. `None` for plugins before 1.2 and for every unknown path.
/// Only the reserved segment itself ignores ASCII case (FR-13.28); the rest must match exactly.
pub fn file(api: (u64, u64), path: &str) -> Option<KitFile> {
    if !is_api_1_2(api) {
        return None;
    }
    let (_reserved, rest) = path.split_once('/')?;
    let rest = rest.strip_prefix(V1)?;
    FILES
        .iter()
        .find(|(name, _)| *name == rest)
        .map(|(_, file)| *file)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::permissions::{API_1_0, API_1_1};

    #[test]
    fn serves_every_kit_file_with_its_type() {
        for (path, mime) in [
            ("_mumcord/v1/kit.css", "text/css; charset=utf-8"),
            (
                "_mumcord/v1/fonts/instrument-sans-latin-wght-normal.woff2",
                "font/woff2",
            ),
            (
                "_mumcord/v1/fonts/bricolage-grotesque-latin-wght-normal.woff2",
                "font/woff2",
            ),
            ("_mumcord/v1/fonts/OFL.txt", "text/plain; charset=utf-8"),
        ] {
            let found = file(API_1_2, path).unwrap();
            assert_eq!(found.mime, mime, "{path}");
            assert!(!found.body.is_empty(), "{path}");
        }
        // The reserved segment ignores ASCII case like the protocol's check.
        assert!(file(API_1_2, "_MUMCORD/v1/kit.css").is_some());
        assert!(file((1, 3), "_mumcord/v1/kit.css").is_some());
    }

    #[test]
    fn fonts_are_woff2() {
        for name in [
            "_mumcord/v1/fonts/instrument-sans-latin-wght-normal.woff2",
            "_mumcord/v1/fonts/bricolage-grotesque-latin-wght-normal.woff2",
        ] {
            assert!(
                file(API_1_2, name).unwrap().body.starts_with(b"wOF2"),
                "{name}"
            );
        }
    }

    #[test]
    fn older_plugins_and_unknown_paths_get_nothing() {
        for api in [API_1_0, API_1_1, (2, 0), (0, 9)] {
            assert_eq!(file(api, "_mumcord/v1/kit.css"), None, "{api:?}");
        }
        for path in [
            "_mumcord",
            "_mumcord/",
            "_mumcord/kit.css",
            "_mumcord/v1",
            "_mumcord/v1/",
            "_mumcord/v1/KIT.css",
            "_mumcord/V1/kit.css",
            "_mumcord/v2/kit.css",
            "_mumcord/v1/kit.css/",
            "_mumcord/v1/kit.css?x",
            "_mumcord/v1//kit.css",
            "_mumcord/v1/./kit.css",
            "_mumcord/v1/fonts/../kit.css",
            "_mumcord/v1/fonts/",
            "_mumcord/v1/fonts/missing.woff2",
            "_mumcord/v1/index.html",
        ] {
            assert_eq!(file(API_1_2, path), None, "{path}");
        }
    }
}
