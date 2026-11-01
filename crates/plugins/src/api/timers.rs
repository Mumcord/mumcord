//! `timers` (FR-6.21). One shared scheduler thread for all plugins pushes `timer` events into
//! each plugin's queue, so a plugin never blocks while waiting. The thread only runs while some
//! timer is active. Every plugin instance owns a [`TimerSet`]; when the instance goes away
//! (stop, crash, restart) its timers go with it.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::timers::{Host, TimerId, add_to_linker};
use crate::runtime::wit::events::Event;
use crate::runtime::wit::types::Error;
use crate::runtime::{EventQueue, HostApi, PluginState};

/// Shortest interval; shorter requests are raised to it.
pub const MIN_INTERVAL: Duration = Duration::from_millis(250);
/// Active timers per plugin instance.
pub const MAX_TIMERS: usize = 32;

pub(crate) struct TimersApi;

impl HostApi for TimersApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn start(&mut self, interval_ms: u32, repeat: bool) -> Result<TimerId, Error> {
        self.timers
            .start(Duration::from_millis(interval_ms.into()), repeat)
    }

    fn cancel(&mut self, id: TimerId) {
        self.timers.cancel(id);
    }
}

/// The scheduler shared by all plugins; lives in [`crate::HostContext`].
#[derive(Debug, Default)]
pub struct Timers {
    shared: Arc<Shared>,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<(u64, TimerId), Entry>,
    next_owner: u64,
    running: bool,
}

#[derive(Debug)]
struct Entry {
    queue: Weak<EventQueue>,
    due: Instant,
    interval: Duration,
    repeat: bool,
}

impl Timers {
    /// A fresh set of timers for one plugin instance that delivers into `queue`.
    pub(crate) fn set(&self, queue: Weak<EventQueue>) -> TimerSet {
        let mut state = self.shared.lock();
        state.next_owner += 1;
        TimerSet {
            shared: Arc::clone(&self.shared),
            owner: state.next_owner,
            queue,
            next_id: 0,
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The timers of one plugin instance; cancels them all when dropped.
pub(crate) struct TimerSet {
    shared: Arc<Shared>,
    owner: u64,
    queue: Weak<EventQueue>,
    next_id: TimerId,
}

impl TimerSet {
    fn start(&mut self, interval: Duration, repeat: bool) -> Result<TimerId, Error> {
        let interval = interval.max(MIN_INTERVAL);
        let mut state = self.shared.lock();
        let owner = self.owner;
        if state.entries.keys().filter(|(o, _)| *o == owner).count() >= MAX_TIMERS {
            return Err(Error::QuotaExceeded);
        }
        // Ids are not reused while a timer with that id is still active.
        let id = loop {
            self.next_id = self.next_id.wrapping_add(1);
            if !state.entries.contains_key(&(owner, self.next_id)) {
                break self.next_id;
            }
        };
        state.entries.insert(
            (owner, id),
            Entry {
                queue: Weak::clone(&self.queue),
                due: Instant::now() + interval,
                interval,
                repeat,
            },
        );
        if !state.running {
            state.running = true;
            let shared = Arc::clone(&self.shared);
            if let Err(error) = std::thread::Builder::new()
                .name("mumcord-plugin-timers".into())
                .spawn(move || run(&shared))
            {
                state.running = false;
                state.entries.remove(&(owner, id));
                tracing::error!(%error, "could not start the plugin timer thread");
                return Err(Error::QuotaExceeded);
            }
        }
        drop(state);
        self.shared.changed.notify_one();
        Ok(id)
    }

    fn cancel(&mut self, id: TimerId) {
        self.shared.lock().entries.remove(&(self.owner, id));
    }
}

impl Drop for TimerSet {
    fn drop(&mut self) {
        let owner = self.owner;
        self.shared.lock().entries.retain(|(o, _), _| *o != owner);
    }
}

/// The scheduler loop. Ends when no timer is left; the next `start` begins a new one.
fn run(shared: &Shared) {
    let mut state = shared.lock();
    loop {
        let Some(next) = state.entries.values().map(|entry| entry.due).min() else {
            state.running = false;
            return;
        };
        let now = Instant::now();
        if next > now {
            state = shared
                .changed
                .wait_timeout(state, next - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            continue;
        }
        state.entries.retain(|&(_, id), entry| {
            if entry.due > now {
                return true;
            }
            let Some(queue) = entry.queue.upgrade() else {
                return false;
            };
            queue.push(Event::Timer(id));
            // Missed ticks are not caught up: a late timer fires once, then a full interval later.
            let due = entry.due + entry.interval;
            entry.due = if due <= now {
                now + entry.interval
            } else {
                due
            };
            entry.repeat
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Diagnostics;
    use crate::runtime::Next;

    fn queue() -> Arc<EventQueue> {
        Arc::new(EventQueue::new(Arc::new(Diagnostics::default())))
    }

    fn next_timer(queue: &EventQueue) -> TimerId {
        match queue.pop() {
            Next::Event(event) => match *event {
                Event::Timer(id) => id,
                _ => panic!("expected a timer event"),
            },
            _ => panic!("expected a timer event"),
        }
    }

    #[test]
    fn one_shot_and_repeating_timers_fire_into_the_queue() {
        let timers = Timers::default();
        let queue = queue();
        let mut set = timers.set(Arc::downgrade(&queue));
        let started = Instant::now();
        let once = set
            .start(Duration::from_millis(1), false)
            .expect("test value");
        assert_eq!(next_timer(&queue), once);
        assert!(started.elapsed() >= MIN_INTERVAL, "raised to 250 ms");
        let every = set
            .start(Duration::from_millis(250), true)
            .expect("test value");
        assert_ne!(every, once);
        assert_eq!(next_timer(&queue), every);
        assert_eq!(next_timer(&queue), every);
        set.cancel(every);
        assert!(timers.shared.lock().entries.is_empty());
    }

    #[test]
    fn at_most_32_active_timers_per_instance() {
        let timers = Timers::default();
        let queue = queue();
        let mut set = timers.set(Arc::downgrade(&queue));
        for _ in 0..MAX_TIMERS {
            set.start(Duration::from_secs(60), true)
                .expect("test value");
        }
        assert_eq!(
            set.start(Duration::from_secs(60), true),
            Err(Error::QuotaExceeded)
        );
        let mut other = timers.set(Arc::downgrade(&queue));
        assert!(other.start(Duration::from_secs(60), true).is_ok());
        set.cancel(1);
        assert!(set.start(Duration::from_secs(60), true).is_ok());
    }

    #[test]
    fn dropping_the_set_cancels_its_timers() {
        let timers = Timers::default();
        let queue = queue();
        let mut set = timers.set(Arc::downgrade(&queue));
        set.start(Duration::from_secs(60), true)
            .expect("test value");
        let mut other = timers.set(Arc::downgrade(&queue));
        other
            .start(Duration::from_secs(60), true)
            .expect("test value");
        drop(set);
        assert_eq!(timers.shared.lock().entries.len(), 1);
    }
}
