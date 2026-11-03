//! Screen share encoder probe results (spec 07, FR-7.55), cached per machine so the hardware
//! probe and smoke tests do not run on every start. The cache is only valid for the driver
//! and app version it was measured with.

use rusqlite::params;

use crate::ScreenOptimize;
use crate::db::Store;
use crate::error::Result;

/// What the probe found for one encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderEntry {
    /// Encoder id as chosen by the screen share engine, for example "av1-nvenc".
    pub encoder: String,
    /// The probe found the encoder.
    pub available: bool,
    /// A smoke test encoded a few real frames with it.
    pub smoke_ok: bool,
}

/// How long the software encoder needed in its self-test for one resolution and frame rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvtSelftest {
    /// Frame height in pixels.
    pub resolution: u32,
    pub fps: u32,
    pub optimize: ScreenOptimize,
    pub svt_selftest_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderCache {
    /// Graphics driver version string the results belong to.
    pub driver_id: String,
    pub app_version: String,
    pub encoders: Vec<EncoderEntry>,
    pub selftests: Vec<SvtSelftest>,
}

impl EncoderCache {
    /// Whether the results still apply to this driver and app version. A different driver or a
    /// Mumcord update means the probe has to run again.
    pub fn is_current(&self, driver_id: &str, app_version: &str) -> bool {
        self.driver_id == driver_id && self.app_version == app_version
    }
}

impl Store {
    /// The cached probe results, or `None` if none were saved yet.
    pub fn encoder_cache(&self) -> Result<Option<EncoderCache>> {
        let mut meta = self
            .conn
            .prepare("SELECT driver_id, app_version FROM encoder_cache_meta WHERE id = 1")?;
        let Some((driver_id, app_version)) = meta
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .next()
            .transpose()?
        else {
            return Ok(None);
        };
        let encoders = self
            .conn
            .prepare("SELECT encoder, available, smoke_ok FROM encoder_cache ORDER BY encoder")?
            .query_map([], |row| {
                Ok(EncoderEntry {
                    encoder: row.get(0)?,
                    available: row.get(1)?,
                    smoke_ok: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let selftests = self
            .conn
            .prepare(
                "SELECT resolution, fps, optimize, svt_selftest_ms FROM encoder_selftest
                 ORDER BY resolution, fps, optimize",
            )?
            .query_map([], |row| {
                Ok(SvtSelftest {
                    resolution: row.get(0)?,
                    fps: row.get(1)?,
                    optimize: optimize_from_sql(&row.get::<_, String>(2)?),
                    svt_selftest_ms: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(EncoderCache {
            driver_id,
            app_version,
            encoders,
            selftests,
        }))
    }

    /// Replaces the cached probe results as a whole.
    pub fn save_encoder_cache(&self, cache: &EncoderCache) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(
            "DELETE FROM encoder_cache_meta; DELETE FROM encoder_cache; DELETE FROM encoder_selftest;",
        )?;
        tx.execute(
            "INSERT INTO encoder_cache_meta (id, driver_id, app_version) VALUES (1, ?1, ?2)",
            params![cache.driver_id, cache.app_version],
        )?;
        for e in &cache.encoders {
            tx.execute(
                "INSERT INTO encoder_cache (encoder, available, smoke_ok) VALUES (?1, ?2, ?3)
                 ON CONFLICT (encoder) DO UPDATE SET available = excluded.available,
                     smoke_ok = excluded.smoke_ok",
                params![e.encoder, e.available, e.smoke_ok],
            )?;
        }
        for t in &cache.selftests {
            tx.execute(
                "INSERT INTO encoder_selftest (resolution, fps, optimize, svt_selftest_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (resolution, fps, optimize) DO UPDATE SET
                     svt_selftest_ms = excluded.svt_selftest_ms",
                params![
                    t.resolution,
                    t.fps,
                    optimize_to_sql(t.optimize),
                    t.svt_selftest_ms
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets the cached results ("Check again").
    pub fn clear_encoder_cache(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM encoder_cache_meta; DELETE FROM encoder_cache; DELETE FROM encoder_selftest;",
        )?;
        Ok(())
    }
}

fn optimize_to_sql(optimize: ScreenOptimize) -> &'static str {
    match optimize {
        ScreenOptimize::Balanced => "balanced",
        ScreenOptimize::Text => "text",
        ScreenOptimize::Motion => "motion",
    }
}

// The table's CHECK constraint only admits the three values written above.
fn optimize_from_sql(value: &str) -> ScreenOptimize {
    match value {
        "text" => ScreenOptimize::Text,
        "motion" => ScreenOptimize::Motion,
        _ => ScreenOptimize::Balanced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;

    fn sample() -> EncoderCache {
        EncoderCache {
            driver_id: "nvidia 570.1".into(),
            app_version: "0.7.0".into(),
            encoders: vec![
                EncoderEntry {
                    encoder: "av1-nvenc".into(),
                    available: true,
                    smoke_ok: false,
                },
                EncoderEntry {
                    encoder: "vp9-vaapi".into(),
                    available: false,
                    smoke_ok: false,
                },
            ],
            selftests: vec![
                SvtSelftest {
                    resolution: 1080,
                    fps: 30,
                    optimize: ScreenOptimize::Balanced,
                    svt_selftest_ms: 21,
                },
                SvtSelftest {
                    resolution: 1080,
                    fps: 30,
                    optimize: ScreenOptimize::Text,
                    svt_selftest_ms: 84,
                },
            ],
        }
    }

    #[test]
    fn empty_until_saved_then_round_trips() {
        let (_dir, store) = temp_store();
        assert_eq!(store.encoder_cache().expect("load"), None);
        store.save_encoder_cache(&sample()).expect("save");
        assert_eq!(store.encoder_cache().expect("load"), Some(sample()));
    }

    #[test]
    fn saving_replaces_the_previous_results() {
        let (_dir, store) = temp_store();
        store.save_encoder_cache(&sample()).expect("save");
        let newer = EncoderCache {
            driver_id: "nvidia 575.2".into(),
            encoders: Vec::new(),
            selftests: Vec::new(),
            ..sample()
        };
        store.save_encoder_cache(&newer).expect("save again");
        assert_eq!(store.encoder_cache().expect("load"), Some(newer));
    }

    #[test]
    fn clear_forgets_everything_and_staleness_is_detected() {
        let (_dir, store) = temp_store();
        store.save_encoder_cache(&sample()).expect("save");
        let cache = store.encoder_cache().expect("load").expect("some");
        assert!(cache.is_current("nvidia 570.1", "0.7.0"));
        assert!(!cache.is_current("nvidia 575.2", "0.7.0"));
        assert!(!cache.is_current("nvidia 570.1", "0.7.1"));
        store.clear_encoder_cache().expect("clear");
        assert_eq!(store.encoder_cache().expect("load"), None);
    }
}
