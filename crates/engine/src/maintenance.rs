//! Automatic clean-up of local data (QA.42): cached link previews older than the configured
//! age and chat history older than the configured age. Runs at start, then periodically, and
//! right after the settings change; the file and database work stays off the UI thread.

use std::time::Duration;

use mumcord_store::UnixMillis;

use crate::chat::now_millis;
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, lock};

const INTERVAL: Duration = Duration::from_secs(60 * 60);
/// History is pruned on every Nth round, so once a day with the hourly [`INTERVAL`].
const HISTORY_EVERY: u64 = 24;
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1000;

/// Messages sent before this moment are older than `days` days.
pub(crate) fn history_cutoff(now: UnixMillis, days: u32) -> UnixMillis {
    now.saturating_sub(i64::from(days).saturating_mul(MILLIS_PER_DAY))
}

impl Engine {
    pub(crate) fn start_maintenance(&self) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            let mut interval = tokio::time::interval(INTERVAL);
            let mut round = 0u64;
            loop {
                interval.tick().await;
                engine
                    .run_maintenance(round.is_multiple_of(HISTORY_EVERY))
                    .await;
                round += 1;
            }
        });
    }

    /// Runs a full clean-up without waiting for it (after the settings changed).
    pub(crate) fn run_maintenance_in_background(&self) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            engine.run_maintenance(true).await;
        });
    }

    async fn run_maintenance(&self, history: bool) {
        let engine = self.clone();
        let done = tokio::task::spawn_blocking(move || engine.maintenance_blocking(history)).await;
        if let Err(e) = done {
            tracing::warn!(error = %e, "the automatic clean-up stopped unexpectedly");
        }
    }

    fn maintenance_blocking(&self, history: bool) {
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings for the clean-up failed");
                return;
            }
        };
        let previews = self.link_previews();
        previews.set_max_age_days(settings.preview_cache_days);
        let freed = previews.prune_expired();
        if freed > 0 {
            tracing::debug!(bytes = freed, "deleted old cached link previews");
        }
        if let (true, Some(days)) = (history, settings.history_max_age_days) {
            let cutoff = history_cutoff(now_millis(), days);
            match lock(&self.store_handle()).delete_messages_before(cutoff) {
                Ok(0) => {}
                Ok(count) => tracing::debug!(count, "deleted old chat history"),
                Err(e) => tracing::warn!(error = %e, "deleting old chat history failed"),
            }
        }
    }

    /// "Clear now": deletes all cached link previews, thumbnails and images; returns the
    /// bytes freed.
    pub async fn preview_cache_clear(&self) -> EngineResult<u64> {
        let engine = self.clone();
        tokio::task::spawn_blocking(move || engine.link_previews().clear_cache())
            .await
            .map_err(|e| EngineError::new(ErrorKind::Internal, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_counts_whole_days_back() {
        let now = 100 * MILLIS_PER_DAY;
        assert_eq!(history_cutoff(now, 30), 70 * MILLIS_PER_DAY);
        assert_eq!(history_cutoff(5, 7), 5 - 7 * MILLIS_PER_DAY);
        assert_eq!(history_cutoff(i64::MIN, u32::MAX), i64::MIN);
    }
}
