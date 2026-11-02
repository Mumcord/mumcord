//! Per-plugin diagnostics counters (FR-6.8), shown in Settings → Plugins → plugin → Diagnostics.
//! Written by the plugin's thread and the host without locks; read as a snapshot.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Default)]
pub struct Diagnostics {
    cpu_micros: AtomicU64,
    memory_bytes: AtomicU64,
    peak_memory_bytes: AtomicU64,
    dropped_events: AtomicU64,
    dropped_messages: AtomicU64,
    restarts: AtomicU64,
}

/// The counters at one moment. `cpu_time_ms` is the time spent inside calls into the plugin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DiagSnapshot {
    pub cpu_time_ms: u64,
    pub memory_bytes: u64,
    pub peak_memory_bytes: u64,
    pub dropped_events: u64,
    pub dropped_messages: u64,
    pub restarts: u64,
}

impl Diagnostics {
    pub(crate) fn add_cpu(&self, spent: Duration) {
        let micros = u64::try_from(spent.as_micros()).unwrap_or(u64::MAX);
        self.cpu_micros.fetch_add(micros, Ordering::Relaxed);
    }

    pub(crate) fn set_memory(&self, bytes: usize) {
        let bytes = bytes as u64;
        self.memory_bytes.store(bytes, Ordering::Relaxed);
        self.peak_memory_bytes.fetch_max(bytes, Ordering::Relaxed);
    }

    pub(crate) fn events_dropped(&self, count: usize) {
        self.dropped_events
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// A plugin message was dropped (rate limit, size, expiry); counted by the messaging layer.
    pub fn message_dropped(&self) {
        self.dropped_messages.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn restarted(&self) {
        self.restarts.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> DiagSnapshot {
        DiagSnapshot {
            cpu_time_ms: self.cpu_micros.load(Ordering::Relaxed) / 1000,
            memory_bytes: self.memory_bytes.load(Ordering::Relaxed),
            peak_memory_bytes: self.peak_memory_bytes.load(Ordering::Relaxed),
            dropped_events: self.dropped_events.load(Ordering::Relaxed),
            dropped_messages: self.dropped_messages.load(Ordering::Relaxed),
            restarts: self.restarts.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_keeps_the_memory_peak() {
        let diag = Diagnostics::default();
        diag.add_cpu(Duration::from_micros(1500));
        diag.add_cpu(Duration::from_micros(600));
        diag.set_memory(4096);
        diag.set_memory(1024);
        diag.events_dropped(3);
        diag.message_dropped();
        diag.restarted();
        assert_eq!(
            diag.snapshot(),
            DiagSnapshot {
                cpu_time_ms: 2,
                memory_bytes: 1024,
                peak_memory_bytes: 4096,
                dropped_events: 3,
                dropped_messages: 1,
                restarts: 1,
            }
        );
    }
}
