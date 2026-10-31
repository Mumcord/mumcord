//! Audio profiles per channel (issue #171, FR-3.62): local overrides of Music quality, low-latency
//! mode and the bitrate cap that apply while the user is in that channel. Mumble servers cannot
//! store them, so they live here, per saved server. A profile is keyed by the channel id and also
//! remembers the channel path, so it still matches after the channel was renamed (same id) or
//! deleted and created again (same path, new id).

use rusqlite::params;

use crate::db::Store;
use crate::error::Result;
use crate::servers::ServerId;

/// The three overrides of a profile; `None` keeps the global setting ("default").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelAudioProfile {
    pub music_quality: Option<bool>,
    pub low_latency: Option<bool>,
    /// Own bitrate cap in kbit/s.
    pub bitrate_kbps: Option<u32>,
}

/// A stored profile with the channel it was saved for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedChannelAudio {
    pub channel_id: u32,
    /// Channel path below the root, joined with `/`, as it was when the profile was saved.
    pub channel_path: String,
    pub profile: ChannelAudioProfile,
}

/// The stored profile that belongs to the channel `channel_id` at `channel_path`: one saved for
/// exactly that channel first, then one saved under the same path (the channel was recreated),
/// then one saved for the same id (the channel was renamed or moved).
pub fn resolve_channel_audio<'a>(
    saved: &'a [SavedChannelAudio],
    channel_id: u32,
    channel_path: &str,
) -> Option<&'a SavedChannelAudio> {
    saved
        .iter()
        .find(|s| s.channel_id == channel_id && s.channel_path == channel_path)
        .or_else(|| saved.iter().find(|s| s.channel_path == channel_path))
        .or_else(|| saved.iter().find(|s| s.channel_id == channel_id))
}

impl Store {
    /// Every profile saved for `server`.
    pub fn channel_audio_profiles(&self, server: ServerId) -> Result<Vec<SavedChannelAudio>> {
        let mut stmt = self.conn.prepare(
            "SELECT channel_id, channel_path, music_quality, low_latency, bitrate_kbps
             FROM channel_audio_profile WHERE server_id = ?1 ORDER BY channel_id",
        )?;
        let rows = stmt.query_map([server], |row| {
            Ok(SavedChannelAudio {
                channel_id: row.get(0)?,
                channel_path: row.get(1)?,
                profile: ChannelAudioProfile {
                    music_quality: row.get(2)?,
                    low_latency: row.get(3)?,
                    bitrate_kbps: row.get(4)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The profile of the channel `channel_id` at `channel_path`, see [`resolve_channel_audio`].
    pub fn channel_audio_profile(
        &self,
        server: ServerId,
        channel_id: u32,
        channel_path: &str,
    ) -> Result<Option<ChannelAudioProfile>> {
        let saved = self.channel_audio_profiles(server)?;
        Ok(resolve_channel_audio(&saved, channel_id, channel_path).map(|s| s.profile))
    }

    /// Saves the profile of a channel. It replaces any profile that matched the channel by id or
    /// by path, so the stored id and path follow the channel's current ones.
    pub fn set_channel_audio_profile(
        &self,
        server: ServerId,
        channel_id: u32,
        channel_path: &str,
        profile: &ChannelAudioProfile,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        delete_matching(&tx, server, channel_id, channel_path)?;
        tx.execute(
            "INSERT INTO channel_audio_profile
                 (server_id, channel_id, channel_path, music_quality, low_latency, bitrate_kbps)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                server,
                channel_id,
                channel_path,
                profile.music_quality,
                profile.low_latency,
                profile.bitrate_kbps
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Removes the profile of a channel ("Remove profile"): the channel uses the global settings
    /// again. Removing a channel without a profile is a no-op.
    pub fn remove_channel_audio_profile(
        &self,
        server: ServerId,
        channel_id: u32,
        channel_path: &str,
    ) -> Result<()> {
        delete_matching(&self.conn, server, channel_id, channel_path)
    }
}

fn delete_matching(
    conn: &rusqlite::Connection,
    server: ServerId,
    channel_id: u32,
    channel_path: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM channel_audio_profile
         WHERE server_id = ?1 AND (channel_id = ?2 OR channel_path = ?3)",
        params![server, channel_id, channel_path],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewServer;
    use crate::db::tests::temp_store;

    const MUSIC: ChannelAudioProfile = ChannelAudioProfile {
        music_quality: Some(true),
        low_latency: None,
        bitrate_kbps: Some(256),
    };
    const JAM: ChannelAudioProfile = ChannelAudioProfile {
        music_quality: None,
        low_latency: Some(true),
        bitrate_kbps: None,
    };

    fn server(store: &Store, name: &str) -> ServerId {
        store
            .add_server(&NewServer::new(name, name, "u"))
            .expect("add")
            .id
    }

    #[test]
    fn saves_replaces_and_removes_per_server() {
        let (_dir, store) = temp_store();
        let a = server(&store, "a");
        let b = server(&store, "b");
        assert_eq!(
            store.channel_audio_profile(a, 5, "Music").expect("get"),
            None
        );
        store
            .set_channel_audio_profile(a, 5, "Music", &MUSIC)
            .expect("set");
        assert_eq!(
            store.channel_audio_profile(a, 5, "Music").expect("get"),
            Some(MUSIC)
        );
        assert_eq!(
            store
                .channel_audio_profile(b, 5, "Music")
                .expect("other server"),
            None
        );
        store
            .set_channel_audio_profile(a, 5, "Music", &JAM)
            .expect("replace");
        assert_eq!(
            store.channel_audio_profiles(a).expect("list"),
            [SavedChannelAudio {
                channel_id: 5,
                channel_path: "Music".into(),
                profile: JAM,
            }]
        );
        store
            .remove_channel_audio_profile(a, 5, "Music")
            .expect("remove");
        store
            .remove_channel_audio_profile(a, 5, "Music")
            .expect("no-op");
        assert!(store.channel_audio_profiles(a).expect("list").is_empty());
    }

    #[test]
    fn a_renamed_or_recreated_channel_still_matches() {
        let (_dir, store) = temp_store();
        let a = server(&store, "a");
        store
            .set_channel_audio_profile(a, 5, "Music", &MUSIC)
            .expect("set");
        store
            .set_channel_audio_profile(a, 7, "Jam", &JAM)
            .expect("set");
        // Renamed: same id, new path.
        assert_eq!(
            store.channel_audio_profile(a, 5, "Music Room").expect("id"),
            Some(MUSIC)
        );
        // Recreated: same path, new id.
        assert_eq!(
            store.channel_audio_profile(a, 12, "Jam").expect("path"),
            Some(JAM)
        );
        // An exact match wins over a path match of another channel.
        store
            .set_channel_audio_profile(a, 9, "Lobby", &JAM)
            .expect("set");
        let saved = store.channel_audio_profiles(a).expect("list");
        assert_eq!(
            resolve_channel_audio(&saved, 9, "Lobby").map(|s| s.channel_id),
            Some(9)
        );
        assert_eq!(
            store.channel_audio_profile(a, 3, "Other").expect("none"),
            None
        );
    }

    #[test]
    fn saving_follows_the_channel_and_drops_stale_matches() {
        let (_dir, store) = temp_store();
        let a = server(&store, "a");
        store
            .set_channel_audio_profile(a, 5, "Music", &MUSIC)
            .expect("set");
        // Saved again after the channel was recreated with id 12: one row with the new id.
        store
            .set_channel_audio_profile(a, 12, "Music", &JAM)
            .expect("set");
        assert_eq!(
            store.channel_audio_profiles(a).expect("list"),
            [SavedChannelAudio {
                channel_id: 12,
                channel_path: "Music".into(),
                profile: JAM,
            }]
        );
        // Removing after a rename finds the profile by id.
        store
            .remove_channel_audio_profile(a, 12, "Music Room")
            .expect("remove");
        assert!(store.channel_audio_profiles(a).expect("list").is_empty());
    }

    #[test]
    fn profiles_go_with_their_server() {
        let (_dir, store) = temp_store();
        let a = server(&store, "a");
        store
            .set_channel_audio_profile(a, 5, "Music", &MUSIC)
            .expect("set");
        store
            .remove_server(a, &crate::SecretStore::in_memory())
            .expect("remove server");
        assert!(store.channel_audio_profiles(a).expect("list").is_empty());
    }
}
