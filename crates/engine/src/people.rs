//! Per-person local settings (FR-1.19) and profile cards (FR-1.18).
//!
//! Volume and local mute are stored by certificate hash per server, so they follow a person
//! across renames and reconnects. People without a certificate are adjusted for the session
//! only.

use std::collections::HashMap;

use mumcord_protocol::User;
use mumcord_store::{PersonSetting, ServerId, Store};

use crate::session::{EngineError, EngineResult, ProfileView, convert::sanitize_html};

/// Session-only settings of people without a certificate.
#[derive(Debug, Default)]
pub(crate) struct People {
    by_session: HashMap<u32, PersonSetting>,
}

impl People {
    /// The local setting that applies to `user`.
    pub fn setting(&self, store: &Store, server: ServerId, user: &User) -> PersonSetting {
        match &user.cert_hash {
            Some(hash) => store
                .person_setting(server, hash)
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "reading person setting failed");
                    None
                })
                .unwrap_or_default(),
            None => self
                .by_session
                .get(&user.session)
                .copied()
                .unwrap_or_default(),
        }
    }

    /// Stores a changed setting under the person's certificate, or for the session.
    pub fn save(
        &mut self,
        store: &Store,
        server: ServerId,
        user: &User,
        setting: PersonSetting,
    ) -> EngineResult<()> {
        if setting.volume > 200 {
            return Err(EngineError::invalid("volume must be 0–200"));
        }
        match &user.cert_hash {
            Some(hash) => store
                .set_person_setting(server, hash, &setting)
                .map_err(crate::session::store_error),
            None => {
                self.by_session.insert(user.session, setting);
                Ok(())
            }
        }
    }

    pub fn forget(&mut self, session: u32) {
        self.by_session.remove(&session);
    }

    pub fn clear(&mut self) {
        self.by_session.clear();
    }
}

/// Whether the mixer must be told about this setting (a fresh mixer starts at the defaults).
pub(crate) fn is_default(setting: PersonSetting) -> bool {
    setting == PersonSetting::default()
}

/// Profile card data from the protocol state; `None` fields that are announced by hash only
/// must be requested first (see [`missing_blobs`]).
pub(crate) fn profile(user: &User) -> ProfileView {
    ProfileView {
        comment: user
            .comment
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(sanitize_html),
        avatar: user.texture.as_deref().and_then(avatar_data_url),
    }
}

/// (texture, comment) still to fetch from the server.
pub(crate) fn missing_blobs(user: &User) -> (bool, bool) {
    (
        user.texture.is_none() && user.texture_hash.is_some(),
        user.comment.is_none() && user.comment_hash.is_some(),
    )
}

/// Encodes an avatar image as a `data:` URL. Only common image formats are accepted; the
/// legacy raw-pixel textures of very old clients are not shown.
pub(crate) fn avatar_data_url(image: &[u8]) -> Option<String> {
    let mime = if image.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if image.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if image.starts_with(b"GIF87a") || image.starts_with(b"GIF89a") {
        "image/gif"
    } else if image.len() > 12 && &image[..4] == b"RIFF" && &image[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return None;
    };
    Some(format!("data:{mime};base64,{}", base64(image)))
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(session: u32, cert: Option<&str>) -> User {
        User {
            session,
            cert_hash: cert.map(str::to_owned),
            ..User::default()
        }
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn avatar_formats() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert!(avatar_data_url(png).is_some_and(|u| u.starts_with("data:image/png;base64,")));
        assert_eq!(avatar_data_url(&[0u8; 100]), None);
    }

    #[test]
    fn settings_follow_certificate_across_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        let server = store
            .add_server(&mumcord_store::NewServer::new("t", "localhost", "me"))
            .expect("server")
            .id;
        let mut people = People::default();
        let quiet = PersonSetting {
            volume: 50,
            local_mute: true,
        };

        people
            .save(&store, server, &user(5, Some("abc")), quiet)
            .expect("save");
        // Same certificate, new session (renamed and reconnected): still applied.
        assert_eq!(people.setting(&store, server, &user(9, Some("abc"))), quiet);

        // Without certificate: only for this session.
        people
            .save(&store, server, &user(6, None), quiet)
            .expect("save");
        assert_eq!(people.setting(&store, server, &user(6, None)), quiet);
        people.forget(6);
        assert!(is_default(people.setting(&store, server, &user(6, None))));

        assert!(
            people
                .save(
                    &store,
                    server,
                    &user(5, Some("abc")),
                    PersonSetting {
                        volume: 201,
                        local_mute: false
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn profile_sanitizes_comment() {
        let mut u = user(1, None);
        u.comment = Some("<i>hi</i><script>x</script>".into());
        let p = profile(&u);
        assert_eq!(p.comment.as_deref(), Some("<i>hi</i>"));
        assert_eq!(p.avatar, None);
    }
}
