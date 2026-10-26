//! Both ends of a full-image transfer, independent of the server connection: messages go out
//! through a [`Link`] and come in through [`Transfers::on_message`], so two instances can be
//! wired together in tests.
//!
//! Sending: requests are served one at a time in arrival order, a few per person and minute,
//! and only for images this client sent itself. Chunks are paced below the server's plugin
//! message limit (4 per second by default; it silently drops anything faster) with room left
//! for other plugin data. Receiving: one request per transfer, with timeouts for a missing
//! answer, a long queue and a stalled transfer; chunks must arrive in order and the result must
//! match the announced size and hash.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;

use super::check::MAX_BYTES;
use super::wire::{CHUNK_LEN, FullRef, Msg};
use crate::session::lock;

/// Requests served per requester within [`REQUEST_WINDOW`]; more are ignored unanswered.
const MAX_REQUESTS: usize = 6;
const REQUEST_WINDOW: Duration = Duration::from_secs(60);
/// Requests waiting behind the running transfer; more are answered "unavailable".
const MAX_QUEUE: usize = 4;
/// Transfers this client receives at the same time.
const MAX_DOWNLOADS: usize = 3;

/// Sends messages to other clients.
pub(crate) trait Link: Send + Sync + 'static {
    /// Sends `msg` to the session `to`; `false` when it could not be sent.
    fn send(&self, to: u32, msg: &Msg) -> bool;
}

/// Looks up the bytes of an image this client sent itself (blocking file access).
pub(crate) type Originals = Arc<dyn Fn(&[u8; 32]) -> Option<Vec<u8>> + Send + Sync>;

/// Timing of transfers; tests use shorter values.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// Gap between two messages of an upload (and after an "unavailable" answer).
    pub pace: Duration,
    /// How long a request may stay unanswered.
    pub reply: Duration,
    /// How long a queued request may wait for its transfer to start.
    pub queued: Duration,
    /// How long a running transfer may stall between chunks.
    pub chunk: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            // About 2.9 messages per second: below the server's 4, leaving room for other
            // plugin data (hello, now playing).
            pace: Duration::from_millis(350),
            reply: Duration::from_secs(10),
            // Long enough to wait behind one transfer of the largest size.
            queued: Duration::from_secs(6 * 60),
            chunk: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FetchError {
    #[error("too many transfers at once")]
    Busy,
    #[error("the request could not be sent")]
    NotSent,
    #[error("the sender did not answer in time")]
    Timeout,
    #[error("the sender left or the connection ended")]
    Gone,
    #[error("the sender does not have the image")]
    Unavailable,
    #[error("the sender broke the transfer protocol")]
    Protocol,
    #[error("the data does not match the announced image")]
    Mismatch,
    #[error("cancelled")]
    Cancelled,
}

struct Job {
    to: u32,
    id: u32,
    hash: [u8; 32],
}

#[derive(Default)]
struct State {
    /// Bumped when the connection changes; tasks of an older one stop.
    generation: u64,
    next_id: u32,
    /// Running downloads by (sender session, transfer id).
    downloads: HashMap<(u32, u32), mpsc::UnboundedSender<Msg>>,
    /// Wake-ups to stop running downloads, with the hash of the image they fetch.
    cancels: HashMap<(u32, u32), ([u8; 32], Arc<Notify>)>,
    queue: VecDeque<Job>,
    /// Requester of the running upload, while one runs.
    active: Option<u32>,
    /// Whether an upload task runs.
    uploading: bool,
    /// Recent request times per requester.
    requests: HashMap<u32, VecDeque<Instant>>,
}

pub(crate) struct Transfers {
    state: Mutex<State>,
    timing: Timing,
}

impl Transfers {
    pub(crate) fn new(timing: Timing) -> Self {
        Self {
            state: Mutex::new(State::default()),
            timing,
        }
    }

    /// The connection ended or changed: drops every transfer.
    pub(crate) fn reset(&self) {
        let mut state = lock(&self.state);
        let generation = state.generation + 1;
        *state = State {
            generation,
            next_id: state.next_id,
            ..State::default()
        };
    }

    /// A person left: their requests are dropped and downloads from them fail.
    pub(crate) fn forget(&self, session: u32) {
        let mut state = lock(&self.state);
        state.queue.retain(|job| job.to != session);
        state.downloads.retain(|(from, _), _| *from != session);
        state.requests.remove(&session);
        if state.active == Some(session) {
            state.active = None;
        }
    }

    /// A message from another client. Must run inside the Tokio runtime (uploads are tasks).
    pub(crate) fn on_message(
        self: &Arc<Self>,
        from: u32,
        msg: Msg,
        link: Arc<dyn Link>,
        originals: Originals,
    ) {
        match msg {
            Msg::Get { id, hash } => self.on_get(from, id, hash, link, originals),
            other => {
                let state = lock(&self.state);
                if let Some(tx) = state.downloads.get(&(from, other.id())) {
                    // A closed channel means the download just ended; nothing to do.
                    let _ = tx.send(other);
                }
            }
        }
    }

    fn on_get(
        self: &Arc<Self>,
        from: u32,
        id: u32,
        hash: [u8; 32],
        link: Arc<dyn Link>,
        originals: Originals,
    ) {
        let now = Instant::now();
        let mut state = lock(&self.state);
        let recent = state.requests.entry(from).or_default();
        while recent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= REQUEST_WINDOW)
        {
            recent.pop_front();
        }
        if recent.len() >= MAX_REQUESTS {
            tracing::debug!("full image: ignoring a request over the rate limit");
            return;
        }
        recent.push_back(now);
        let already = state.active == Some(from) || state.queue.iter().any(|j| j.to == from);
        if already || state.queue.len() >= MAX_QUEUE {
            drop(state);
            link.send(from, &Msg::Unavailable { id });
            return;
        }
        state.queue.push_back(Job { to: from, id, hash });
        if state.uploading {
            drop(state);
            link.send(from, &Msg::Queued { id });
            return;
        }
        state.uploading = true;
        let generation = state.generation;
        drop(state);
        tokio::spawn(self.clone().upload(generation, link, originals));
    }

    /// Serves the queue until it is empty or the connection changes.
    async fn upload(self: Arc<Self>, generation: u64, link: Arc<dyn Link>, originals: Originals) {
        loop {
            let job = {
                let mut state = lock(&self.state);
                if state.generation != generation {
                    return;
                }
                match state.queue.pop_front() {
                    Some(job) => {
                        state.active = Some(job.to);
                        job
                    }
                    None => {
                        state.active = None;
                        state.uploading = false;
                        return;
                    }
                }
            };
            let lookup = originals.clone();
            let hash = job.hash;
            let bytes = tokio::task::spawn_blocking(move || lookup(&hash))
                .await
                .ok()
                .flatten()
                .filter(|b| b.len() <= MAX_BYTES && FullRef::of(b).hash == hash);
            let Some(bytes) = bytes else {
                link.send(job.to, &Msg::Unavailable { id: job.id });
                tokio::time::sleep(self.timing.pace).await;
                continue;
            };
            let size = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            if !link.send(job.to, &Msg::Start { id: job.id, size }) {
                continue;
            }
            tracing::debug!(bytes = size, "full image: upload started");
            for (index, chunk) in (0u32..).zip(bytes.chunks(CHUNK_LEN)) {
                tokio::time::sleep(self.timing.pace).await;
                if !self.still_serving(generation, job.to) {
                    break;
                }
                let msg = Msg::Chunk {
                    id: job.id,
                    index,
                    data: chunk.to_vec(),
                };
                if !link.send(job.to, &msg) {
                    break;
                }
            }
            tokio::time::sleep(self.timing.pace).await;
        }
    }

    /// Stops the running downloads of this image (the viewer closed). Chunks that still
    /// arrive find no download and are ignored.
    pub(crate) fn cancel(&self, hash: &[u8; 32]) {
        let state = lock(&self.state);
        for (key, notify) in state.cancels.values() {
            if key == hash {
                notify.notify_one();
            }
        }
    }

    fn still_serving(&self, generation: u64, to: u32) -> bool {
        let state = lock(&self.state);
        state.generation == generation && state.active == Some(to)
    }

    /// Fetches the image `full` from the session `from`. `progress` gets the received and
    /// total bytes after the start and every chunk. The bytes match `full`'s size and hash but
    /// are not checked as an image yet.
    pub(crate) async fn fetch(
        self: &Arc<Self>,
        link: Arc<dyn Link>,
        from: u32,
        full: FullRef,
        progress: impl Fn(u32, u32),
    ) -> Result<Vec<u8>, FetchError> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancelled = Arc::new(Notify::new());
        let id = {
            let mut state = lock(&self.state);
            if state.downloads.len() >= MAX_DOWNLOADS {
                return Err(FetchError::Busy);
            }
            state.next_id = state.next_id.wrapping_add(1);
            let id = state.next_id;
            state.downloads.insert((from, id), tx);
            state
                .cancels
                .insert((from, id), (full.hash, cancelled.clone()));
            id
        };
        let _registered = Registered {
            transfers: self,
            key: (from, id),
        };
        if !link.send(
            from,
            &Msg::Get {
                id,
                hash: full.hash,
            },
        ) {
            return Err(FetchError::NotSent);
        }
        let total = usize::try_from(full.size).map_err(|_| FetchError::Mismatch)?;
        let mut wait = self.timing.reply;
        let mut data: Option<Vec<u8>> = None;
        loop {
            let msg = tokio::select! {
                () = cancelled.notified() => return Err(FetchError::Cancelled),
                msg = tokio::time::timeout(wait, rx.recv()) => {
                    msg.map_err(|_| FetchError::Timeout)?.ok_or(FetchError::Gone)?
                }
            };
            match (msg, data.as_mut()) {
                (Msg::Queued { .. }, None) => wait = self.timing.queued,
                (Msg::Start { size, .. }, None) => {
                    if size != full.size {
                        return Err(FetchError::Mismatch);
                    }
                    data = Some(Vec::with_capacity(total));
                    wait = self.timing.chunk;
                    progress(0, full.size);
                }
                (
                    Msg::Chunk {
                        index, data: chunk, ..
                    },
                    Some(data),
                ) => {
                    let expected_len = CHUNK_LEN.min(total - data.len());
                    let expected_index = data.len() / CHUNK_LEN;
                    if usize::try_from(index).ok() != Some(expected_index)
                        || chunk.len() != expected_len
                    {
                        return Err(FetchError::Protocol);
                    }
                    data.extend_from_slice(&chunk);
                    progress(u32::try_from(data.len()).unwrap_or(u32::MAX), full.size);
                    if data.len() == total {
                        let bytes = std::mem::take(data);
                        return if full.matches(&bytes) {
                            Ok(bytes)
                        } else {
                            Err(FetchError::Mismatch)
                        };
                    }
                }
                (Msg::Unavailable { .. }, _) => return Err(FetchError::Unavailable),
                _ => return Err(FetchError::Protocol),
            }
        }
    }
}

/// Removes a download's entry however the fetch ends.
struct Registered<'a> {
    transfers: &'a Transfers,
    key: (u32, u32),
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut state = lock(&self.transfers.state);
        state.downloads.remove(&self.key);
        state.cancels.remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    //! Two simulated clients wired together through an in-memory link.

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ALICE: u32 = 1;
    const BOB: u32 = 2;
    const CAROL: u32 = 3;

    fn timing() -> Timing {
        Timing {
            pace: Duration::from_millis(1),
            reply: Duration::from_millis(500),
            queued: Duration::from_secs(5),
            chunk: Duration::from_millis(500),
        }
    }

    /// Messages to `peer` are delivered to it as coming from `me`; `tamper` may change them.
    struct Wire {
        me: u32,
        peers: Mutex<HashMap<u32, Arc<Client>>>,
        sent: AtomicUsize,
        tamper: Box<dyn Fn(Msg) -> Option<Msg> + Send + Sync>,
    }

    impl Link for Wire {
        fn send(&self, to: u32, msg: &Msg) -> bool {
            self.sent.fetch_add(1, Ordering::Relaxed);
            // Through the real encoding, as on the server.
            let Some(msg) = Msg::decode(&msg.encode()).and_then(|m| (self.tamper)(m)) else {
                return true;
            };
            let Some(peer) = lock(&self.peers).get(&to).cloned() else {
                return false;
            };
            peer.transfers
                .on_message(self.me, msg, peer.wire.clone(), peer.originals.clone());
            true
        }
    }

    struct Client {
        transfers: Arc<Transfers>,
        wire: Arc<Wire>,
        originals: Originals,
    }

    fn client(
        me: u32,
        own: Vec<Vec<u8>>,
        tamper: impl Fn(Msg) -> Option<Msg> + Send + Sync + 'static,
    ) -> Arc<Client> {
        let originals: Originals =
            Arc::new(move |hash| own.iter().find(|b| FullRef::of(b).hash == *hash).cloned());
        Arc::new(Client {
            transfers: Arc::new(Transfers::new(timing())),
            wire: Arc::new(Wire {
                me,
                peers: Mutex::new(HashMap::new()),
                sent: AtomicUsize::new(0),
                tamper: Box::new(tamper),
            }),
            originals,
        })
    }

    fn connect(clients: &[&Arc<Client>]) {
        for a in clients {
            for b in clients {
                if !Arc::ptr_eq(a, b) {
                    lock(&a.wire.peers).insert(b.wire.me, (*b).clone());
                }
            }
        }
    }

    fn image(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 % 251) as u8).collect()
    }

    async fn fetch(
        receiver: &Arc<Client>,
        from: u32,
        full: FullRef,
    ) -> (Result<Vec<u8>, FetchError>, Vec<(u32, u32)>) {
        let seen = Mutex::new(Vec::new());
        let result = receiver
            .transfers
            .fetch(receiver.wire.clone(), from, full, |r, t| {
                lock(&seen).push((r, t));
            })
            .await;
        (result, seen.into_inner().expect("progress"))
    }

    #[tokio::test]
    async fn full_image_arrives_in_chunks_with_progress() {
        let picture = image(CHUNK_LEN * 3 + 17);
        let alice = client(ALICE, vec![picture.clone()], Some);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);

        let (result, progress) = fetch(&bob, ALICE, FullRef::of(&picture)).await;
        assert_eq!(result, Ok(picture.clone()));
        let total = u32::try_from(picture.len()).expect("size");
        assert_eq!(progress.first(), Some(&(0, total)));
        assert_eq!(progress.last(), Some(&(total, total)));
        assert_eq!(progress.len(), 1 + 4);
        // Start + 4 chunks from Alice; one request from Bob.
        assert_eq!(alice.wire.sent.load(Ordering::Relaxed), 5);
        assert_eq!(bob.wire.sent.load(Ordering::Relaxed), 1);
        assert!(lock(&bob.transfers.state).downloads.is_empty());
    }

    #[tokio::test]
    async fn unknown_images_are_unavailable() {
        let alice = client(ALICE, vec![image(10)], Some);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let (result, _) = fetch(&bob, ALICE, FullRef::of(&image(11))).await;
        assert_eq!(result, Err(FetchError::Unavailable));
    }

    #[tokio::test]
    async fn a_silent_or_offline_sender_times_out_or_fails() {
        let alice = client(ALICE, vec![image(10)], |_| None);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let full = FullRef::of(&image(10));
        // Alice is not a Mumcord client (her answers never arrive).
        assert_eq!(fetch(&bob, ALICE, full).await.0, Err(FetchError::Timeout));
        // Nobody with that session.
        assert_eq!(fetch(&bob, CAROL, full).await.0, Err(FetchError::NotSent));
    }

    #[tokio::test]
    async fn corrupted_data_is_rejected() {
        let picture = image(CHUNK_LEN + 5);
        let flip = |msg: Msg| match msg {
            Msg::Chunk {
                id,
                index,
                mut data,
            } => {
                data[0] ^= 1;
                Some(Msg::Chunk { id, index, data })
            }
            other => Some(other),
        };
        let alice = client(ALICE, vec![picture.clone()], flip);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let (result, _) = fetch(&bob, ALICE, FullRef::of(&picture)).await;
        assert_eq!(result, Err(FetchError::Mismatch));
    }

    #[tokio::test]
    async fn wrong_sizes_and_order_are_rejected() {
        let picture = image(CHUNK_LEN * 2);
        let bigger = |msg: Msg| match msg {
            Msg::Start { id, size } => Some(Msg::Start { id, size: size + 1 }),
            other => Some(other),
        };
        let alice = client(ALICE, vec![picture.clone()], bigger);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let full = FullRef::of(&picture);
        assert_eq!(fetch(&bob, ALICE, full).await.0, Err(FetchError::Mismatch));

        let skip_first = |msg: Msg| match msg {
            Msg::Chunk { index: 0, .. } => None,
            other => Some(other),
        };
        let alice = client(ALICE, vec![picture.clone()], skip_first);
        connect(&[&alice, &bob]);
        assert_eq!(fetch(&bob, ALICE, full).await.0, Err(FetchError::Protocol));
    }

    #[tokio::test]
    async fn requests_queue_one_at_a_time_and_are_rate_limited() {
        let picture = image(CHUNK_LEN * 4);
        let alice = client(ALICE, vec![picture.clone()], Some);
        let bob = client(BOB, vec![], Some);
        let carol = client(CAROL, vec![], Some);
        connect(&[&alice, &bob, &carol]);
        let full = FullRef::of(&picture);

        // Bob and Carol at once: one is served after the other, both get the image.
        let (b, c) = tokio::join!(fetch(&bob, ALICE, full), fetch(&carol, ALICE, full));
        assert_eq!(b.0.as_ref(), Ok(&picture));
        assert_eq!(c.0.as_ref(), Ok(&picture));

        // A second request while the first is running is refused.
        let (first, second) = tokio::join!(fetch(&bob, ALICE, full), fetch(&bob, ALICE, full));
        assert!(
            [&first.0, &second.0].contains(&&Err(FetchError::Unavailable)),
            "{first:?} {second:?}"
        );

        // Requests beyond the per-minute limit go unanswered.
        for _ in 0..MAX_REQUESTS {
            let _ = fetch(&carol, ALICE, full).await;
        }
        assert_eq!(fetch(&carol, ALICE, full).await.0, Err(FetchError::Timeout));
    }

    #[tokio::test]
    async fn a_leaving_sender_fails_the_download() {
        let picture = image(CHUNK_LEN * 50);
        let alice = client(ALICE, vec![picture.clone()], Some);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let bob_transfers = bob.transfers.clone();
        let leave = async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            bob_transfers.forget(ALICE);
            alice.transfers.forget(BOB);
        };
        let (result, ()) = tokio::join!(fetch(&bob, ALICE, FullRef::of(&picture)), leave);
        assert_eq!(result.0, Err(FetchError::Gone));
    }

    #[tokio::test]
    async fn cancel_stops_the_download_and_ignores_late_chunks() {
        let picture = image(CHUNK_LEN * 3);
        let full = FullRef::of(&picture);
        let alice = client(ALICE, vec![picture], |msg| match msg {
            // The sender stalls after the first chunk, so the download is still running.
            Msg::Chunk { index, .. } if index > 0 => None,
            other => Some(other),
        });
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let canceller = bob.transfers.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            canceller.cancel(&full.hash);
        });
        let started = Instant::now();
        let (result, _) = fetch(&bob, ALICE, full).await;
        assert!(matches!(result, Err(FetchError::Cancelled)));
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "no timeout wait"
        );
        assert!(lock(&bob.transfers.state).downloads.is_empty());
        assert!(lock(&bob.transfers.state).cancels.is_empty());
        // A chunk arriving after the cancel finds no download and is dropped.
        bob.transfers.on_message(
            ALICE,
            Msg::Chunk {
                id: 1,
                index: 1,
                data: vec![0; CHUNK_LEN],
            },
            bob.wire.clone(),
            bob.originals.clone(),
        );
        assert!(lock(&bob.transfers.state).downloads.is_empty());
    }

    #[tokio::test]
    async fn concurrent_downloads_are_capped() {
        let alice = client(ALICE, vec![], |_| None);
        let bob = client(BOB, vec![], Some);
        connect(&[&alice, &bob]);
        let full = FullRef::of(&image(10));
        let results = futures_join(&bob, full).await;
        assert!(results.contains(&Err(FetchError::Busy)), "{results:?}");
    }

    async fn futures_join(bob: &Arc<Client>, full: FullRef) -> Vec<Result<Vec<u8>, FetchError>> {
        let (a, b, c, d) = tokio::join!(
            fetch(bob, ALICE, full),
            fetch(bob, ALICE, full),
            fetch(bob, ALICE, full),
            fetch(bob, ALICE, full)
        );
        vec![a.0, b.0, c.0, d.0]
    }
}
