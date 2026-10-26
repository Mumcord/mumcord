//! Connection quality, client and OS of people from Mumble UserStats (FR-2.25, FR-2.27).
//!
//! The UI says which people of a server it shows (`user_stats_watch`); one task per server
//! then requests their UserStats one at a time. Murmur does not rate-limit UserStats, but the
//! pacing keeps them from crowding chat and voice on the control channel: at most one request
//! every 2 s per server, people in the user's channel first, each person at most every 30 s.
//!
//! What the server returns depends on the user's permissions: ping and transport for anyone
//! in a channel the user may enter, loss and bandwidth only for the own channel, client and
//! OS only for the own session or to admins. Missing fields stay `None`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mumcord_protocol::proto;
use mumcord_protocol::{Handle, MumbleVersion};

use crate::ServerId;
use crate::session::{Engine, EngineEvent, EngineResult, SessionCtx, VoiceTransportKind, lock};

/// Smallest gap between two requests on one server.
const REQUEST_GAP: Duration = Duration::from_secs(2);
/// How long a person's figures count as fresh.
const REFRESH: Duration = Duration::from_secs(30);
/// Cap for the client and OS text another client sends about itself.
const MAX_TEXT_CHARS: usize = 64;

/// One person's figures as the UI shows them; `None` when the server did not return them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserStatsView {
    pub session: u32,
    /// Average ping of the transport the person's voice uses.
    pub ping_ms: Option<f32>,
    pub transport: Option<VoiceTransportKind>,
    /// Lost voice packets in both directions since the person connected, 0–100.
    pub loss_percent: Option<f32>,
    /// Late voice packets in both directions since the person connected, 0–100.
    pub late_percent: Option<f32>,
    pub bandwidth_kbps: Option<u32>,
    /// Client release text, e.g. "Mumble 1.5.735" or "Mumcord 1.2.0" (untrusted plain text).
    pub client_release: Option<String>,
    /// Protocol version of the client, e.g. "1.5.735".
    pub client_version: Option<String>,
    /// Operating system and its version (untrusted plain text).
    pub os: Option<String>,
    pub os_version: Option<String>,
}

/// Client and OS of a person; sent only with full (not `stats_only`) answers.
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientInfo {
    release: Option<String>,
    version: Option<String>,
    os: Option<String>,
    os_version: Option<String>,
}

/// Which person to ask next, and when.
#[derive(Debug, Default)]
struct Scheduler {
    watched: Vec<u32>,
    /// When each person was last requested (kept after they leave the view so re-showing them
    /// does not bypass the 30 s limit; cleared when they leave the server).
    requested: HashMap<u32, Instant>,
    last_request: Option<Instant>,
}

impl Scheduler {
    /// The next person to request and whether a short (`stats_only`) answer is enough because
    /// client and OS were asked for already. `place` says whether a session is in the user's
    /// channel, or `None` if it is not on the server.
    fn next(&mut self, now: Instant, place: impl Fn(u32) -> Option<bool>) -> Option<(u32, bool)> {
        if self
            .last_request
            .is_some_and(|t| now.saturating_duration_since(t) < REQUEST_GAP)
        {
            return None;
        }
        let (session, last) = self
            .watched
            .iter()
            .filter_map(|&s| place(s).map(|own| (s, own, self.requested.get(&s).copied())))
            .filter(|(_, _, last)| last.is_none_or(|t| now.saturating_duration_since(t) >= REFRESH))
            // Own channel first, then whoever waited longest (never asked before anyone).
            .min_by_key(|&(s, own, last)| (!own, last, s))
            .map(|(s, _, last)| (s, last))?;
        self.requested.insert(session, now);
        self.last_request = Some(now);
        Some((session, last.is_some()))
    }
}

#[derive(Default)]
struct ServerStats {
    scheduler: Scheduler,
    clients: HashMap<u32, ClientInfo>,
    /// Whether the request task of this server runs.
    running: bool,
}

/// UserStats state of all servers.
#[derive(Default)]
pub(crate) struct UserStatsHub {
    servers: HashMap<ServerId, ServerStats>,
}

/// A UserStats answer arrived: merge in the known client and tell the UI.
pub(crate) fn on_stats(hub: &Mutex<UserStatsHub>, ctx: &SessionCtx, msg: &proto::UserStats) {
    let Some(session) = msg.session else {
        return;
    };
    let client = {
        let mut hub = lock(hub);
        let server = hub.servers.entry(ctx.server_id).or_default();
        if let Some(version) = &msg.version {
            server.clients.insert(session, client_info(version));
        }
        server.clients.get(&session).cloned().unwrap_or_default()
    };
    ctx.events
        .emit(EngineEvent::UserStatsChanged(view(session, msg, client)));
}

/// The person left the server: their session id may be given to someone else.
pub(crate) fn on_user_removed(hub: &Mutex<UserStatsHub>, server_id: ServerId, session: u32) {
    if let Some(server) = lock(hub).servers.get_mut(&server_id) {
        server.clients.remove(&session);
        server.scheduler.requested.remove(&session);
    }
}

impl Engine {
    /// Sets the people of `server_id` the UI shows; their UserStats are requested at a paced
    /// rate and arrive as [`EngineEvent::UserStatsChanged`]. An empty list stops the requests.
    pub fn user_stats_watch(&self, server_id: ServerId, sessions: Vec<u32>) -> EngineResult<()> {
        self.session_ctx_for(server_id)?;
        let start = {
            let mut hub = lock(self.user_stats_state());
            let server = hub.servers.entry(server_id).or_default();
            server.scheduler.watched = sessions;
            let start = !server.running && !server.scheduler.watched.is_empty();
            server.running |= start;
            start
        };
        if start {
            let engine = self.clone();
            self.runtime()
                .spawn(async move { engine.run_user_stats(server_id).await });
        }
        Ok(())
    }

    async fn run_user_stats(&self, server_id: ServerId) {
        let mut tick = tokio::time::interval(REQUEST_GAP);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut connection: Option<Handle> = None;
        loop {
            tick.tick().await;
            let Ok(ctx) = self.session_ctx_for(server_id) else {
                lock(self.user_stats_state()).servers.remove(&server_id);
                return;
            };
            let reconnected = connection.as_ref().is_some_and(|h| !h.is_connected());
            connection = Some(ctx.handle.clone());
            let next = ctx.handle.with_state(|state| {
                let own_channel = state
                    .session
                    .and_then(|s| state.users.get(&s))
                    .map(|u| u.channel_id);
                let mut hub = lock(self.user_stats_state());
                let server = hub.servers.entry(server_id).or_default();
                if reconnected {
                    // Session ids of the new connection mean other people.
                    server.clients.clear();
                    server.scheduler.requested.clear();
                }
                if server.scheduler.watched.is_empty() {
                    server.running = false;
                    return None;
                }
                Some(server.scheduler.next(Instant::now(), |s| {
                    state
                        .users
                        .get(&s)
                        .map(|u| Some(u.channel_id) == own_channel)
                }))
            });
            let Some(next) = next else {
                return;
            };
            if let Some((session, stats_only)) = next
                && let Err(e) = ctx.handle.request_user_stats(session, stats_only)
            {
                tracing::warn!(error = %e, "requesting UserStats failed");
            }
        }
    }
}

fn view(session: u32, msg: &proto::UserStats, client: ClientInfo) -> UserStatsView {
    // Like Mumble: the UDP ping when the person has UDP, otherwise TCP. The server only relays
    // what the person's client reported in its own pings and sends 0 for anything never
    // reported, so 0 means unknown, not a perfect link.
    let measured = |ping: Option<f32>| ping.filter(|ms| ms.is_finite() && *ms > 0.0);
    let udp = msg.udp_packets.is_some_and(|n| n > 0);
    let (transport, ping_ms) = match (
        measured(msg.udp_ping_avg).filter(|_| udp),
        measured(msg.tcp_ping_avg),
    ) {
        (Some(ping), _) => (Some(VoiceTransportKind::Udp), Some(ping)),
        (None, Some(ping)) => (Some(VoiceTransportKind::Tcp), Some(ping)),
        (None, None) => (None, None),
    };
    let (loss_percent, late_percent) = packet_shares(msg);
    UserStatsView {
        session,
        ping_ms,
        transport,
        loss_percent,
        late_percent,
        // The server counts bytes per second.
        bandwidth_kbps: msg.bandwidth.map(|b| b.saturating_mul(8) / 1000),
        client_release: client.release,
        client_version: client.version,
        os: client.os,
        os_version: client.os_version,
    }
}

/// Lost and late shares of both directions; `None` when the server sent no packet counts
/// (person outside the user's channel).
fn packet_shares(msg: &proto::UserStats) -> (Option<f32>, Option<f32>) {
    let directions = [&msg.from_client, &msg.from_server];
    if directions.iter().all(|d| d.is_none()) {
        return (None, None);
    }
    let sum = |f: fn(&proto::user_stats::Stats) -> Option<u32>| -> u64 {
        directions
            .iter()
            .filter_map(|d| d.as_ref().and_then(f))
            .map(u64::from)
            .sum()
    };
    let good = sum(|s| s.good);
    let late = sum(|s| s.late);
    let lost = sum(|s| s.lost);
    let total = good + late + lost;
    // No counted packets yet (or a client that never reports them) says nothing about loss.
    if total == 0 {
        return (None, None);
    }
    #[allow(clippy::cast_precision_loss)] // packet counts are far below f64 precision
    let share = |n: u64| (n as f64 * 100.0 / total as f64) as f32;
    (Some(share(lost)), Some(share(late)))
}

fn client_info(version: &proto::Version) -> ClientInfo {
    let number = version
        .version_v2
        .map(MumbleVersion::from_v2)
        .or_else(|| version.version_v1.map(MumbleVersion::from_v1))
        .filter(|v| (v.major, v.minor, v.patch) != (0, 0, 0));
    ClientInfo {
        release: version.release.as_deref().and_then(plain_text),
        version: number.map(|v| v.to_string()),
        os: version.os.as_deref().and_then(plain_text),
        os_version: version.os_version.as_deref().and_then(plain_text),
    }
}

/// Untrusted text from another client as one short plain line; `None` if nothing is left.
fn plain_text(text: &str) -> Option<String> {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean: String = clean.trim().chars().take(MAX_TEXT_CHARS).collect();
    let clean = clean.trim_end();
    (!clean.is_empty()).then(|| clean.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler(watched: &[u32]) -> Scheduler {
        Scheduler {
            watched: watched.to_vec(),
            ..Scheduler::default()
        }
    }

    /// Sessions 1–2 in another channel, 3–4 in the user's, 9 not on the server.
    fn place(s: u32) -> Option<bool> {
        match s {
            1 | 2 => Some(false),
            3 | 4 => Some(true),
            _ => None,
        }
    }

    #[test]
    fn own_channel_first_one_request_per_gap() {
        let t0 = Instant::now();
        let mut s = scheduler(&[9, 1, 2, 4, 3]);
        assert_eq!(s.next(t0, place), Some((3, false)));
        assert_eq!(s.next(t0 + Duration::from_millis(1999), place), None);
        let mut t = t0 + REQUEST_GAP;
        let mut order = Vec::new();
        while let Some((session, _)) = s.next(t, place) {
            order.push(session);
            t += REQUEST_GAP;
        }
        assert_eq!(order, [4, 1, 2]);
    }

    #[test]
    fn each_person_at_most_every_30_seconds() {
        let t0 = Instant::now();
        let mut s = scheduler(&[1]);
        assert_eq!(s.next(t0, place), Some((1, false)));
        assert_eq!(s.next(t0 + Duration::from_secs(29), place), None);
        assert_eq!(s.next(t0 + REFRESH, place), Some((1, true)));
    }

    #[test]
    fn rewatching_does_not_bypass_the_limit() {
        let t0 = Instant::now();
        let mut s = scheduler(&[1]);
        s.next(t0, place);
        s.watched = Vec::new();
        s.watched = vec![1];
        assert_eq!(s.next(t0 + Duration::from_secs(10), place), None);
    }

    #[test]
    fn stale_people_before_fresh_ones_in_the_same_channel() {
        let t0 = Instant::now();
        let mut s = scheduler(&[1]);
        s.next(t0, place);
        s.watched = vec![1, 2];
        let t = t0 + REFRESH;
        assert_eq!(s.next(t, place), Some((2, false)));
        assert_eq!(s.next(t + REQUEST_GAP, place), Some((1, true)));
    }

    fn stats(good: u32, late: u32, lost: u32) -> proto::user_stats::Stats {
        proto::user_stats::Stats {
            good: Some(good),
            late: Some(late),
            lost: Some(lost),
            resync: Some(0),
        }
    }

    #[test]
    fn full_answer_in_own_channel() {
        let msg = proto::UserStats {
            session: Some(5),
            from_client: Some(stats(90, 5, 5)),
            from_server: Some(stats(100, 0, 0)),
            udp_packets: Some(10),
            udp_ping_avg: Some(23.5),
            tcp_ping_avg: Some(40.0),
            bandwidth: Some(5000),
            ..Default::default()
        };
        let v = view(5, &msg, ClientInfo::default());
        assert_eq!(v.transport, Some(VoiceTransportKind::Udp));
        assert_eq!(v.ping_ms, Some(23.5));
        assert_eq!(v.loss_percent, Some(2.5));
        assert_eq!(v.late_percent, Some(2.5));
        assert_eq!(v.bandwidth_kbps, Some(40));
    }

    #[test]
    fn other_channel_has_ping_only_and_tcp_without_udp() {
        let msg = proto::UserStats {
            session: Some(5),
            udp_packets: Some(0),
            tcp_packets: Some(30),
            udp_ping_avg: Some(0.0),
            tcp_ping_avg: Some(55.0),
            ..Default::default()
        };
        let v = view(5, &msg, ClientInfo::default());
        assert_eq!(v.transport, Some(VoiceTransportKind::Tcp));
        assert_eq!(v.ping_ms, Some(55.0));
        assert_eq!(
            (v.loss_percent, v.late_percent, v.bandwidth_kbps),
            (None, None, None)
        );
        assert_eq!(v.client_release, None);
    }

    #[test]
    fn udp_without_udp_ping_falls_back_to_tcp() {
        let msg = proto::UserStats {
            udp_packets: Some(12),
            udp_ping_avg: Some(0.0),
            tcp_ping_avg: Some(35.0),
            ..Default::default()
        };
        let v = view(5, &msg, ClientInfo::default());
        assert_eq!(
            (v.transport, v.ping_ms),
            (Some(VoiceTransportKind::Tcp), Some(35.0))
        );
    }

    #[test]
    fn missing_or_zero_ping_is_none() {
        for (udp, tcp) in [(None, None), (Some(0.0), Some(0.0)), (Some(f32::NAN), None)] {
            let msg = proto::UserStats {
                udp_packets: Some(3),
                udp_ping_avg: udp,
                tcp_ping_avg: tcp,
                ..Default::default()
            };
            let v = view(5, &msg, ClientInfo::default());
            assert_eq!((v.transport, v.ping_ms), (None, None), "{udp:?} {tcp:?}");
        }
    }

    #[test]
    fn no_packets_yet_is_unknown_loss() {
        let msg = proto::UserStats {
            from_client: Some(stats(0, 0, 0)),
            from_server: Some(stats(0, 0, 0)),
            ..Default::default()
        };
        assert_eq!(packet_shares(&msg), (None, None));
    }

    #[test]
    fn client_and_os_are_cleaned_and_capped() {
        let info = client_info(&proto::Version {
            version_v1: None,
            version_v2: Some(MumbleVersion::from_v1(0x0001_0504).to_v2()),
            release: Some(" Mumble\n1.5.735\u{7} ".to_owned()),
            os: Some("X".repeat(200)),
            os_version: Some("  \t ".to_owned()),
        });
        assert_eq!(info.release.as_deref(), Some("Mumble 1.5.735"));
        assert_eq!(info.version.as_deref(), Some("1.5.4"));
        assert_eq!(info.os.map(|o| o.chars().count()), Some(MAX_TEXT_CHARS));
        assert_eq!(info.os_version, None);
    }

    #[test]
    fn empty_version_shows_no_client() {
        assert_eq!(
            client_info(&proto::Version::default()),
            ClientInfo::default()
        );
    }
}
