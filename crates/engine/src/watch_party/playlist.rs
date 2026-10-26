//! The shared playlist of a watch party (FR-2.69).
//!
//! The list travels as small edit operations, not as a whole: one plugin message holds at most
//! [`PLUGIN_DATA_MAX`] bytes, far less than 50 entries. Every client keeps the entries sorted by a
//! position key (ties broken by entry id), so the same operations give the same order no matter
//! in which order they arrive. A move sets a new key and carries a Lamport stamp; of two moves of
//! one entry the later stamp wins everywhere ("the last change wins"). A removal is final: the id
//! is remembered so a late add or move cannot bring the entry back.
//!
//! Edits are bundled: they wait in an outbox and leave at most once per [`FLUSH_INTERVAL`], as
//! many as fit into one message; the server only allows a few plugin messages per second.

use std::collections::VecDeque;
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, Instant};

use mumcord_protocol::PLUGIN_DATA_MAX;
use serde::{Deserialize, Serialize};

use super::valid_video_id;

/// More entries than this are never added (FR-2.69).
pub const MAX_ENTRIES: usize = 50;
/// Playlist edits leave at most this often.
pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
/// Removed ids remembered against late adds; far more than one list ever holds.
const MAX_REMOVED: usize = 256;

/// One video waiting in the playlist, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub id: u32,
    pub video_id: String,
    /// Session of who added it.
    pub added_by: u32,
    /// Their name when the entry arrived, for when they have left since.
    pub added_by_name: Option<String>,
}

/// A playlist edit as it travels inside a `pl` message. `c` is the Lamport clock of the edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "o", rename_all = "lowercase")]
pub(crate) enum Op {
    Add {
        id: u32,
        v: String,
        k: f64,
        c: u32,
        /// Who added it, when that is not the sender (a latecomer's copy of the list).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<u32>,
    },
    Rm {
        id: u32,
    },
    Mv {
        id: u32,
        k: f64,
        c: u32,
    },
}

#[derive(Debug, Clone)]
struct Entry {
    id: u32,
    video: String,
    by: u32,
    by_name: Option<String>,
    key: f64,
    /// Stamp of the last add or move: (clock, session).
    stamp: (u32, u32),
}

/// What [`Playlist::flush`] wants the caller to do.
#[derive(Debug, PartialEq)]
pub(crate) enum Flush {
    /// Nothing waits.
    Idle,
    /// Edits wait, but the last message left less than [`FLUSH_INTERVAL`] ago.
    Wait(Duration),
    /// Send these ops now; `more` when some did not fit and wait for the next turn.
    Send { ops: Vec<Op>, more: bool },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Playlist {
    /// Always sorted by (key, id).
    entries: Vec<Entry>,
    removed: VecDeque<u32>,
    clock: u32,
    outbox: Vec<Op>,
    last_flush: Option<Instant>,
    /// A task is sending the outbox.
    pub(crate) flushing: bool,
}

impl Playlist {
    pub(crate) fn view(&self) -> Vec<PlaylistEntry> {
        self.entries
            .iter()
            .map(|e| PlaylistEntry {
                id: e.id,
                video_id: e.video.clone(),
                added_by: e.by,
                added_by_name: e.by_name.clone(),
            })
            .collect()
    }

    fn knows(&self, id: u32) -> bool {
        id == 0 || self.removed.contains(&id) || self.entries.iter().any(|e| e.id == id)
    }

    fn tick(&mut self) -> u32 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn see(&mut self, c: u32) {
        self.clock = self.clock.max(c);
    }

    fn sort(&mut self) {
        self.entries
            .sort_by(|a, b| a.key.total_cmp(&b.key).then(a.id.cmp(&b.id)));
    }

    fn forget(&mut self, id: u32) {
        self.entries.retain(|e| e.id != id);
        if !self.removed.contains(&id) {
            if self.removed.len() == MAX_REMOVED {
                self.removed.pop_front();
            }
            self.removed.push_back(id);
        }
    }

    /// A fresh entry id that is not 0 (the video the party started with) and not in use.
    fn new_id(&self, own: u32) -> u32 {
        loop {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u32(own);
            hasher.write_u32(self.clock);
            let id = u32::try_from(hasher.finish() & u64::from(u32::MAX)).unwrap_or(1);
            if !self.knows(id) {
                return id;
            }
        }
    }

    /// The user adds a video at the end; `None` when the list is full or the id is not valid.
    pub(crate) fn add(&mut self, own: u32, own_name: Option<String>, video: &str) -> Option<u32> {
        if self.entries.len() >= MAX_ENTRIES || !valid_video_id(video) {
            return None;
        }
        let id = self.new_id(own);
        let key = self.entries.last().map_or(1.0, |e| e.key + 1.0);
        let c = self.tick();
        self.entries.push(Entry {
            id,
            video: video.to_owned(),
            by: own,
            by_name: own_name,
            key,
            stamp: (c, own),
        });
        self.sort();
        self.outbox.push(Op::Add {
            id,
            v: video.to_owned(),
            k: key,
            c,
            by: None,
        });
        Some(id)
    }

    /// The user removes an entry; false when it is not in the list.
    pub(crate) fn remove(&mut self, id: u32) -> bool {
        if !self.entries.iter().any(|e| e.id == id) {
            return false;
        }
        self.forget(id);
        self.outbox.push(Op::Rm { id });
        true
    }

    /// The user moves an entry to `index` of the list (clamped); false when it is not there.
    pub(crate) fn move_to(&mut self, own: u32, id: u32, index: usize) -> bool {
        let Some(from) = self.entries.iter().position(|e| e.id == id) else {
            return false;
        };
        let mut others: Vec<f64> = self.entries.iter().map(|e| e.key).collect();
        others.remove(from);
        let index = index.min(others.len());
        if index == from {
            return true;
        }
        let key = match (index.checked_sub(1).map(|i| others[i]), others.get(index)) {
            (None, None) => 1.0,
            (None, Some(next)) => next - 1.0,
            (Some(prev), None) => prev + 1.0,
            (Some(prev), Some(next)) => prev + (next - prev) / 2.0,
        };
        let c = self.tick();
        let entry = &mut self.entries[from];
        entry.key = key;
        entry.stamp = (c, own);
        self.sort();
        self.outbox.push(Op::Mv { id, k: key, c });
        true
    }

    /// Takes the first entry out, for "next" and "Skip".
    pub(crate) fn pop_front(&mut self) -> Option<(u32, String)> {
        let first = self.entries.first().map(|e| (e.id, e.video.clone()))?;
        self.forget(first.0);
        Some(first)
    }

    /// Another client went on to entry `id`; it leaves the list for good.
    pub(crate) fn consumed(&mut self, id: u32) {
        self.forget(id);
    }

    /// Applies edits that `sender` made; `name_of` resolves who added an entry. Returns whether
    /// the list changed. Invalid ops are skipped one by one.
    pub(crate) fn apply(
        &mut self,
        sender: u32,
        ops: Vec<Op>,
        name_of: impl Fn(u32) -> Option<String>,
    ) -> bool {
        let mut changed = false;
        for op in ops {
            match op {
                Op::Add { id, v, k, c, by } => {
                    self.see(c);
                    if self.knows(id)
                        || !k.is_finite()
                        || !valid_video_id(&v)
                        || self.entries.len() >= MAX_ENTRIES
                    {
                        continue;
                    }
                    let by = by.unwrap_or(sender);
                    self.entries.push(Entry {
                        id,
                        video: v,
                        by,
                        by_name: name_of(by),
                        key: k,
                        stamp: (c, sender),
                    });
                    changed = true;
                }
                Op::Rm { id } => {
                    changed |= self.entries.iter().any(|e| e.id == id);
                    self.forget(id);
                }
                Op::Mv { id, k, c } => {
                    self.see(c);
                    let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
                        continue;
                    };
                    if k.is_finite() && (c, sender) > entry.stamp {
                        entry.key = k;
                        entry.stamp = (c, sender);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.sort();
        }
        changed
    }

    /// The whole list as adds, for someone who came into the channel later.
    pub(crate) fn snapshot(&self) -> Vec<Op> {
        self.entries
            .iter()
            .map(|e| Op::Add {
                id: e.id,
                v: e.video.clone(),
                k: e.key,
                c: e.stamp.0,
                by: Some(e.by),
            })
            .collect()
    }

    /// The edits that may leave now, bundled into one message.
    pub(crate) fn flush(&mut self, now: Instant, encode: impl Fn(&[Op]) -> Vec<u8>) -> Flush {
        if self.outbox.is_empty() {
            return Flush::Idle;
        }
        if let Some(last) = self.last_flush {
            let due = last + FLUSH_INTERVAL;
            if now < due {
                return Flush::Wait(due - now);
            }
        }
        let rest = split_fitting(&mut self.outbox, &encode);
        let ops = std::mem::replace(&mut self.outbox, rest);
        self.last_flush = Some(now);
        Flush::Send {
            more: !self.outbox.is_empty(),
            ops,
        }
    }
}

/// Splits off the ops after the longest front of `ops` that still encodes into one message; at
/// least one op stays (a single op always fits).
pub(crate) fn split_fitting(ops: &mut Vec<Op>, encode: &impl Fn(&[Op]) -> Vec<u8>) -> Vec<Op> {
    let mut rest = Vec::new();
    while ops.len() > 1 && encode(ops).len() > PLUGIN_DATA_MAX {
        if let Some(op) = ops.pop() {
            rest.push(op);
        }
    }
    rest.reverse();
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "dQw4w9WgXcQ";
    const B: &str = "a-b_c-d_e-f";
    const C: &str = "0123456789a";

    fn ids(list: &Playlist) -> Vec<String> {
        list.view().into_iter().map(|e| e.video_id).collect()
    }

    fn take(list: &mut Playlist) -> Vec<Op> {
        std::mem::take(&mut list.outbox)
    }

    #[test]
    fn local_edits_add_remove_and_reorder() {
        let mut list = Playlist::default();
        let a = list.add(1, Some("Anna".into()), A).expect("added");
        let b = list.add(1, None, B).expect("added");
        let c = list.add(1, None, C).expect("added");
        assert_eq!(ids(&list), [A, B, C]);
        assert!(list.move_to(1, c, 0));
        assert_eq!(ids(&list), [C, A, B]);
        assert!(list.move_to(1, c, 1));
        assert_eq!(ids(&list), [A, C, B]);
        assert!(list.move_to(1, a, 99), "clamped to the end");
        assert_eq!(ids(&list), [C, B, A]);
        assert!(list.remove(b));
        assert!(!list.remove(b));
        assert_eq!(ids(&list), [C, A]);
        assert_eq!(list.view()[1].added_by_name.as_deref(), Some("Anna"));
        assert_eq!(list.pop_front().map(|(_, v)| v).as_deref(), Some(C));
        assert_eq!(ids(&list), [A]);
    }

    #[test]
    fn invalid_videos_and_a_full_list_are_refused() {
        let mut list = Playlist::default();
        assert_eq!(list.add(1, None, "javascript:"), None);
        for _ in 0..MAX_ENTRIES {
            assert!(list.add(1, None, A).is_some());
        }
        assert_eq!(list.add(1, None, A), None);
        assert_eq!(list.view().len(), MAX_ENTRIES);
    }

    #[test]
    fn the_same_edits_in_any_order_give_the_same_list() {
        let mut anna = Playlist::default();
        let mut ben = Playlist::default();
        anna.add(1, None, A);
        ben.add(2, None, B);
        let from_anna = take(&mut anna);
        let from_ben = take(&mut ben);
        anna.apply(2, from_ben.clone(), |_| None);
        ben.apply(1, from_anna.clone(), |_| None);
        assert_eq!(ids(&anna), ids(&ben), "concurrent adds land in one order");

        let first = anna.view()[0].id;
        let second = anna.view()[1].id;
        // Both move entries at once; the later stamp wins for each entry everywhere.
        anna.move_to(1, first, 1);
        ben.move_to(2, first, 1);
        ben.move_to(2, second, 1);
        let from_anna = take(&mut anna);
        let from_ben = take(&mut ben);
        anna.apply(2, from_ben, |_| None);
        ben.apply(1, from_anna, |_| None);
        assert_eq!(ids(&anna), ids(&ben));
    }

    #[test]
    fn removals_are_final_and_bad_ops_are_skipped() {
        let mut list = Playlist::default();
        let ops = vec![
            Op::Add {
                id: 7,
                v: A.into(),
                k: 1.0,
                c: 1,
                by: None,
            },
            Op::Add {
                id: 8,
                v: "../../etc".into(),
                k: 2.0,
                c: 2,
                by: None,
            },
            Op::Add {
                id: 0,
                v: B.into(),
                k: 2.0,
                c: 2,
                by: None,
            },
        ];
        assert!(list.apply(3, ops, |s| (s == 3).then(|| "Cleo".to_owned())));
        assert_eq!(ids(&list), [A]);
        assert_eq!(list.view()[0].added_by, 3);
        assert_eq!(list.view()[0].added_by_name.as_deref(), Some("Cleo"));
        assert!(list.apply(3, vec![Op::Rm { id: 7 }], |_| None));
        let late = vec![
            Op::Add {
                id: 7,
                v: A.into(),
                k: 1.0,
                c: 1,
                by: None,
            },
            Op::Mv {
                id: 7,
                k: 3.0,
                c: 9,
            },
        ];
        assert!(
            !list.apply(3, late, |_| None),
            "a removed entry stays removed"
        );
        assert!(list.view().is_empty());
        assert_eq!(list.clock, 9, "the clock follows what others sent");
    }

    #[test]
    fn a_stale_move_loses() {
        let mut list = Playlist::default();
        let a = list.add(1, None, A).expect("added");
        list.add(1, None, B);
        list.move_to(1, a, 1);
        let stamp = list
            .entries
            .iter()
            .find(|e| e.id == a)
            .expect("entry")
            .stamp;
        assert!(!list.apply(
            0,
            vec![Op::Mv {
                id: a,
                k: -5.0,
                c: stamp.0
            }],
            |_| None
        ));
        assert_eq!(ids(&list), [B, A]);
    }

    #[test]
    fn edits_leave_bundled_and_at_most_once_a_second() {
        let encode = |ops: &[Op]| serde_json::to_vec(ops).unwrap_or_default();
        let mut list = Playlist::default();
        let start = Instant::now();
        assert_eq!(list.flush(start, encode), Flush::Idle);
        for _ in 0..3 {
            list.add(1, None, A);
        }
        let Flush::Send { ops, more } = list.flush(start, encode) else {
            panic!("the first edit leaves at once");
        };
        assert_eq!((ops.len(), more), (3, false), "three edits, one message");
        list.add(1, None, B);
        assert_eq!(
            list.flush(start + Duration::from_millis(400), encode),
            Flush::Wait(Duration::from_millis(600))
        );
        assert!(matches!(
            list.flush(start + FLUSH_INTERVAL, encode),
            Flush::Send { .. }
        ));
    }

    #[test]
    fn a_big_bundle_is_split_to_fit() {
        let encode = |ops: &[Op]| serde_json::to_vec(ops).unwrap_or_default();
        let mut list = Playlist::default();
        for _ in 0..MAX_ENTRIES {
            list.add(u32::MAX, None, A);
        }
        let mut sent = 0;
        let mut now = Instant::now();
        loop {
            match list.flush(now, encode) {
                Flush::Send { ops, more } => {
                    assert!(encode(&ops).len() <= PLUGIN_DATA_MAX);
                    sent += ops.len();
                    if !more {
                        break;
                    }
                }
                other => panic!("unexpected {other:?}"),
            }
            now += FLUSH_INTERVAL;
        }
        assert_eq!(sent, MAX_ENTRIES);
    }
}
