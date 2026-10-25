//! GIF favorites: image links the user saved to send again (FR-1.52, QA.43).

use rusqlite::params;

use crate::db::{Store, UnixMillis};
use crate::error::Result;

/// How many favorites are kept; saving more drops the least recently used ones.
pub const MAX_GIF_FAVORITES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GifFavorite {
    pub url: String,
    pub added_at: UnixMillis,
    pub last_used_at: UnixMillis,
}

impl Store {
    /// All favorites, most recently used first.
    pub fn gif_favorites(&self) -> Result<Vec<GifFavorite>> {
        let mut stmt = self.conn.prepare(
            "SELECT url, added_at, last_used_at FROM gif_favorite
             ORDER BY last_used_at DESC, added_at DESC, url",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GifFavorite {
                url: row.get(0)?,
                added_at: row.get(1)?,
                last_used_at: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Saves `url`; saving a known link only counts as a use. Beyond
    /// [`MAX_GIF_FAVORITES`] the least recently used favorites are dropped.
    pub fn add_gif_favorite(&self, url: &str, now: UnixMillis) -> Result<()> {
        self.conn.execute(
            "INSERT INTO gif_favorite (url, added_at, last_used_at) VALUES (?1, ?2, ?2)
             ON CONFLICT (url) DO UPDATE SET last_used_at = excluded.last_used_at",
            params![url, now],
        )?;
        self.conn.execute(
            "DELETE FROM gif_favorite WHERE url NOT IN (
                 SELECT url FROM gif_favorite
                 ORDER BY last_used_at DESC, added_at DESC, url LIMIT ?1)",
            [MAX_GIF_FAVORITES as i64],
        )?;
        Ok(())
    }

    /// Removes a favorite; an unknown link is a no-op.
    pub fn remove_gif_favorite(&self, url: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM gif_favorite WHERE url = ?1", [url])?;
        Ok(())
    }

    /// Moves a favorite to the front of the order; an unknown link is a no-op.
    pub fn use_gif_favorite(&self, url: &str, now: UnixMillis) -> Result<()> {
        self.conn.execute(
            "UPDATE gif_favorite SET last_used_at = ?2 WHERE url = ?1",
            params![url, now],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(store: &Store) -> Vec<String> {
        store
            .gif_favorites()
            .expect("list")
            .into_iter()
            .map(|f| f.url)
            .collect()
    }

    #[test]
    fn add_remove_and_order_by_last_use() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open");
        assert!(urls(&store).is_empty());
        store.add_gif_favorite("https://a/1.gif", 10).expect("add");
        store.add_gif_favorite("https://a/2.gif", 20).expect("add");
        assert_eq!(urls(&store), ["https://a/2.gif", "https://a/1.gif"]);
        store.use_gif_favorite("https://a/1.gif", 30).expect("use");
        assert_eq!(urls(&store), ["https://a/1.gif", "https://a/2.gif"]);
        // Saving again keeps the original date and only counts as a use.
        store
            .add_gif_favorite("https://a/2.gif", 40)
            .expect("again");
        let list = store.gif_favorites().expect("list");
        assert_eq!(
            (list[0].url.as_str(), list[0].added_at),
            ("https://a/2.gif", 20)
        );
        store
            .remove_gif_favorite("https://a/2.gif")
            .expect("remove");
        store.remove_gif_favorite("https://a/2.gif").expect("no-op");
        assert_eq!(urls(&store), ["https://a/1.gif"]);
    }

    #[test]
    fn keeps_at_most_the_limit_and_drops_the_oldest_use() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open");
        for i in 0..=MAX_GIF_FAVORITES {
            store
                .add_gif_favorite(&format!("https://a/{i}.gif"), i as UnixMillis + 1)
                .expect("add");
        }
        let list = urls(&store);
        assert_eq!(list.len(), MAX_GIF_FAVORITES);
        assert_eq!(list[0], format!("https://a/{MAX_GIF_FAVORITES}.gif"));
        assert!(!list.contains(&"https://a/0.gif".to_owned()));
    }
}
