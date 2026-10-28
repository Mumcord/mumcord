//! Transport choice (UDP or TCP tunnel) and link statistics, as pure logic over a clock passed
//! in by the caller so it can be tested without sockets.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::crypt::CryptStats;

/// Interval of the encrypted UDP connectivity pings.
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(1);
/// UDP counts as broken (voice moves to the TCP tunnel) only when *both* hold: no valid UDP
/// packet (voice or ping reply) for this long, and this many pings in a row went unanswered.
/// The fallback is meant for blocked UDP, not for lossy UDP: with voice flowing, random loss
/// never leaves a gap this long, and even with only pings on the wire (10 % loss each way,
/// ~19 % of round trips lost) five misses in a row are rare. Blocked UDP still falls back in
/// about 6 s.
const UDP_TIMEOUT: Duration = Duration::from_secs(5);
const MISSED_PINGS_TO_FALL_BACK: u32 = 5;
/// Answered pings in a row needed to move back from TCP to UDP after a fallback.
const REPLIES_TO_RECOVER: u32 = 3;
/// Decrypt failures only trigger a resync request after this long without a good packet, and
/// at most once per this interval (same as Mumble's client).
const RESYNC_AFTER: Duration = Duration::from_secs(5);
/// Window for the loss fraction.
const LOSS_WINDOW: Duration = Duration::from_secs(5);
/// Smoothing factor of the RTT average.
const RTT_ALPHA: f64 = 0.25;

/// The path voice currently takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Encrypted UDP straight to the server.
    Udp,
    /// Tunnelled through the TLS control connection (UDP blocked or not working).
    Tcp,
}

/// Link quality for the bitrate controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkStats {
    pub transport: Transport,
    /// Fraction of packets lost over the last few seconds (0.0–1.0): the larger of the
    /// server → client loss seen by the crypto and the share of unanswered UDP pings. Always
    /// 0.0 on the TCP tunnel, which does not lose packets.
    pub loss: f32,
    /// Smoothed UDP ping round-trip time; `None` until measured and while on TCP.
    pub rtt: Option<Duration>,
    /// Raw counters of the decrypting side (as Mumble reports them).
    pub crypt: CryptStats,
    /// Outgoing audio packets dropped so far because they exceeded the voice packet limit.
    pub oversized: u64,
    /// Audio packets sent and received over either transport since the link started.
    pub audio_sent: u64,
    pub audio_received: u64,
}

pub(crate) struct Link {
    transport: Transport,
    ever_udp: bool,
    last_udp: Option<Instant>,
    /// Timestamp of the ping still waiting for a reply.
    outstanding: Option<u64>,
    replies_in_row: u32,
    /// Pings in a row that got no reply (the outstanding one not included).
    missed_in_row: u32,
    rtt: Option<Duration>,
    /// (time, good, lost, pings sent, pings answered) samples for the loss window.
    samples: VecDeque<(Instant, u32, u32, u32, u32)>,
    pings_sent: u32,
    pings_answered: u32,
    last_good_decrypt: Instant,
    last_resync_request: Option<Instant>,
}

impl Link {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            transport: Transport::Tcp,
            ever_udp: false,
            last_udp: None,
            outstanding: None,
            replies_in_row: 0,
            missed_in_row: 0,
            rtt: None,
            samples: VecDeque::new(),
            pings_sent: 0,
            pings_answered: 0,
            last_good_decrypt: now,
            last_resync_request: None,
        }
    }

    pub(crate) fn transport(&self) -> Transport {
        self.transport
    }

    /// A ping with `timestamp` was sent now.
    pub(crate) fn ping_sent(&mut self, timestamp: u64) {
        if self.outstanding.is_some() {
            // The previous ping went unanswered.
            self.replies_in_row = 0;
            self.missed_in_row = self.missed_in_row.saturating_add(1);
        }
        self.outstanding = Some(timestamp);
        self.pings_sent = self.pings_sent.wrapping_add(1);
    }

    /// Any packet that decrypted fine arrived over UDP.
    pub(crate) fn udp_received(&mut self, now: Instant) {
        self.last_udp = Some(now);
        self.last_good_decrypt = now;
    }

    /// A ping reply arrived over UDP; returns the new transport if it changed.
    pub(crate) fn ping_reply(&mut self, timestamp: u64, rtt: Duration) -> Option<Transport> {
        // Any reply, even a late one, shows the path is not blocked.
        self.missed_in_row = 0;
        if self.outstanding != Some(timestamp) {
            // Stale reply of an older ping: proves UDP works but says nothing about the latest.
            return None;
        }
        self.outstanding = None;
        self.pings_answered = self.pings_answered.wrapping_add(1);
        self.replies_in_row += 1;
        self.rtt = Some(match self.rtt {
            Some(avg) => avg.mul_f64(1.0 - RTT_ALPHA) + rtt.mul_f64(RTT_ALPHA),
            None => rtt,
        });
        let needed = if self.ever_udp { REPLIES_TO_RECOVER } else { 1 };
        (self.transport == Transport::Tcp && self.replies_in_row >= needed)
            .then(|| self.switch(Transport::Udp))
    }

    /// Periodic check, called once per [`PING_INTERVAL`] before the next ping is sent; returns
    /// the new transport if UDP stopped working.
    pub(crate) fn tick(&mut self, now: Instant, crypt: CryptStats) -> Option<Transport> {
        self.samples.push_back((
            now,
            crypt.good,
            crypt.lost,
            self.pings_sent,
            self.pings_answered,
        ));
        while self
            .samples
            .front()
            .is_some_and(|s| now.duration_since(s.0) > LOSS_WINDOW)
        {
            self.samples.pop_front();
        }
        let silent = self
            .last_udp
            .is_none_or(|t| now.duration_since(t) >= UDP_TIMEOUT);
        // The outstanding ping was sent a whole interval ago: unanswered by now means lost.
        let missed = self.missed_in_row + u32::from(self.outstanding.is_some());
        (self.transport == Transport::Udp && silent && missed >= MISSED_PINGS_TO_FALL_BACK)
            .then(|| self.switch(Transport::Tcp))
    }

    /// A UDP packet failed to decrypt; `true` if a resync should be requested now.
    pub(crate) fn decrypt_failed(&mut self, now: Instant) -> bool {
        let stale = now.duration_since(self.last_good_decrypt) > RESYNC_AFTER;
        let may_ask = self
            .last_resync_request
            .is_none_or(|t| now.duration_since(t) > RESYNC_AFTER);
        if stale && may_ask {
            self.last_resync_request = Some(now);
            return true;
        }
        false
    }

    pub(crate) fn stats(&self, crypt: CryptStats) -> LinkStats {
        let udp = self.transport == Transport::Udp;
        let loss = match (udp, self.samples.front()) {
            (true, Some(&(_, good0, lost0, sent0, answered0))) => {
                let good = crypt.good.wrapping_sub(good0);
                let lost = crypt.lost.wrapping_sub(lost0);
                let sent = self.pings_sent.wrapping_sub(sent0);
                let answered = self.pings_answered.wrapping_sub(answered0);
                let crypt_loss = ratio(lost, good.saturating_add(lost));
                // The newest ping may still be in flight.
                let unanswered = sent.saturating_sub(answered).saturating_sub(1);
                crypt_loss.max(ratio(unanswered, sent.saturating_sub(1)))
            }
            _ => 0.0,
        };
        LinkStats {
            transport: self.transport,
            loss,
            rtt: if udp { self.rtt } else { None },
            crypt,
            oversized: 0,
            audio_sent: 0,
            audio_received: 0,
        }
    }

    fn switch(&mut self, to: Transport) -> Transport {
        self.transport = to;
        self.replies_in_row = 0;
        self.missed_in_row = 0;
        if to == Transport::Udp {
            self.ever_udp = true;
        }
        to
    }
}

fn ratio(part: u32, total: u32) -> f32 {
    if total == 0 {
        0.0
    } else {
        (f64::from(part) / f64::from(total)) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn stats(good: u32, lost: u32) -> CryptStats {
        CryptStats {
            good,
            lost,
            ..CryptStats::default()
        }
    }

    #[test]
    fn starts_on_tcp_and_moves_to_udp_on_first_reply() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        assert_eq!(l.transport(), Transport::Tcp);
        l.ping_sent(1);
        l.udp_received(t0 + MS);
        assert_eq!(l.ping_reply(1, MS), Some(Transport::Udp));
        assert_eq!(l.stats(stats(1, 0)).rtt, Some(MS));
    }

    #[test]
    fn falls_back_after_silence_and_missed_pings_and_needs_three_replies_to_recover() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        l.ping_sent(0);
        l.udp_received(t0);
        l.ping_reply(0, MS);
        // Silence alone is not enough: no ping went unanswered yet.
        assert_eq!(l.tick(t0 + Duration::from_secs(6), stats(1, 0)), None);
        // Pings go unanswered from now on; only the fifth miss in a row switches.
        let t = t0 + Duration::from_secs(6);
        for ts in 1..=4 {
            l.ping_sent(ts);
            assert_eq!(l.tick(t + Duration::from_secs(ts), stats(1, 0)), None);
        }
        l.ping_sent(5);
        assert_eq!(
            l.tick(t + Duration::from_secs(5), stats(1, 0)),
            Some(Transport::Tcp)
        );
        assert_eq!(l.stats(stats(1, 0)).rtt, None);

        let t = t0 + Duration::from_secs(12);
        for ts in 6..8 {
            l.ping_sent(ts);
            l.udp_received(t);
            assert_eq!(l.ping_reply(ts, MS), None);
        }
        // A missed ping resets the run.
        l.ping_sent(8);
        l.ping_sent(9);
        assert_eq!(l.ping_reply(8, MS), None, "stale reply ignored");
        assert_eq!(l.ping_reply(9, MS), None);
        l.ping_sent(10);
        assert_eq!(l.ping_reply(10, MS), None);
        l.ping_sent(11);
        assert_eq!(l.ping_reply(11, MS), Some(Transport::Udp));
    }

    #[test]
    fn recent_voice_prevents_fallback_despite_missed_pings() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        l.ping_sent(0);
        l.udp_received(t0);
        l.ping_reply(0, MS);
        for ts in 1..=10 {
            let now = t0 + Duration::from_secs(ts);
            l.udp_received(now - 100 * MS);
            assert_eq!(l.tick(now, stats(1, 0)), None, "second {ts}");
            l.ping_sent(ts);
        }
    }

    #[test]
    fn resync_requested_only_when_stale_and_rate_limited() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        assert!(!l.decrypt_failed(t0 + Duration::from_secs(1)));
        assert!(l.decrypt_failed(t0 + Duration::from_secs(6)));
        assert!(!l.decrypt_failed(t0 + Duration::from_secs(8)));
        assert!(l.decrypt_failed(t0 + Duration::from_secs(12)));
        l.udp_received(t0 + Duration::from_secs(13));
        assert!(!l.decrypt_failed(t0 + Duration::from_secs(14)));
    }

    #[test]
    fn loss_fraction_over_window() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        l.ping_sent(1);
        l.udp_received(t0);
        l.ping_reply(1, MS);
        l.tick(t0, stats(100, 0));
        l.tick(t0 + Duration::from_secs(1), stats(150, 10));
        let s = l.stats(stats(190, 10));
        assert!((s.loss - 0.1).abs() < 1e-6, "{}", s.loss);

        // Old samples fall out of the window.
        l.tick(t0 + Duration::from_secs(7), stats(290, 10));
        assert_eq!(l.stats(stats(290, 10)).loss, 0.0);
    }

    #[test]
    fn unanswered_pings_count_as_loss() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        l.ping_sent(1);
        l.udp_received(t0);
        l.ping_reply(1, MS);
        l.tick(t0, stats(1, 0));
        for ts in 2..=5 {
            l.ping_sent(ts);
        }
        l.ping_reply(5, MS);
        // 4 sent since the sample, 1 answered, the newest counts as in flight: 2 of 3 lost.
        let s = l.stats(stats(2, 0));
        assert!(s.loss > 0.6, "{}", s.loss);
    }

    /// Tiny deterministic PRNG (xorshift64*) so the loss simulations are reproducible.
    struct Rng(u64);

    impl Rng {
        fn chance(&mut self, p: f64) -> bool {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
            (x as f64) / ((1u64 << 53) as f64) < p
        }
    }

    /// Drives a [`Link`] like the voice worker does, in 10 ms steps: a tick and a ping every
    /// second (reply after 20 ms), and optionally incoming voice every 20 ms. `loss(t)` is the
    /// drop probability per direction at simulated second `t`. Returns the transport changes
    /// with the time they happened.
    fn simulate(
        seed: u64,
        secs: u64,
        voice: bool,
        loss: impl Fn(f64) -> f64,
    ) -> Vec<(Duration, Transport)> {
        let t0 = Instant::now();
        let mut rng = Rng(seed | 1);
        let mut l = Link::new(t0);
        let mut changes = Vec::new();
        let (mut good, mut lost) = (0u32, 0u32);
        let mut reply_due: Option<(Duration, u64)> = None;
        for step in 0..secs * 100 {
            let at = Duration::from_millis(step * 10);
            let now = t0 + at;
            let p = loss(at.as_secs_f64());
            if let Some((due, ts)) = reply_due
                && due <= at
            {
                reply_due = None;
                if !rng.chance(p) {
                    good += 1;
                    l.udp_received(now);
                    if let Some(to) = l.ping_reply(ts, 20 * MS) {
                        changes.push((at, to));
                    }
                }
            }
            if voice && step % 2 == 0 {
                if rng.chance(p) {
                    lost += 1;
                } else {
                    good += 1;
                    l.udp_received(now);
                }
            }
            if step % 100 == 0 {
                if let Some(to) = l.tick(now, stats(good, lost)) {
                    changes.push((at, to));
                }
                let ts = step;
                l.ping_sent(ts);
                if !rng.chance(p) {
                    reply_due = Some((at + 20 * MS, ts));
                }
            }
        }
        changes
    }

    #[test]
    fn random_loss_with_voice_never_falls_back() {
        for loss in [0.1, 0.3] {
            for seed in 1..=20 {
                let changes = simulate(seed, 600, true, |_| loss);
                // Only the initial switch to UDP once the first ping got through.
                assert!(
                    matches!(changes[..], [(_, Transport::Udp)]),
                    "loss {loss}, seed {seed}: {changes:?}"
                );
            }
        }
    }

    #[test]
    fn ten_percent_loss_without_voice_stays_on_udp() {
        // Only pings on the wire (nobody talks): ~19 % of round trips are lost.
        let mut fallbacks = 0;
        for seed in 1..=20 {
            fallbacks += simulate(seed, 600, false, |_| 0.1)
                .iter()
                .filter(|c| c.1 == Transport::Tcp)
                .count();
        }
        // 200 simulated minutes; the old rule (any 4 s without a valid packet) switched 8 times.
        assert_eq!(fallbacks, 0);
    }

    #[test]
    fn blocked_udp_falls_back_within_six_seconds_and_recovers_after_unblock() {
        const BLOCK: f64 = 30.5;
        const UNBLOCK: f64 = 60.0;
        for voice in [false, true] {
            for seed in 1..=10 {
                let changes = simulate(seed, 90, voice, |t| {
                    if (BLOCK..UNBLOCK).contains(&t) {
                        1.0
                    } else {
                        0.0
                    }
                });
                let [first, down, up] = changes[..] else {
                    panic!("unexpected changes {changes:?}");
                };
                assert_eq!(first.1, Transport::Udp);
                assert_eq!(down.1, Transport::Tcp);
                let after_block = down.0.as_secs_f64() - BLOCK;
                assert!(
                    (5.0..=6.5).contains(&after_block),
                    "fell back {after_block} s after blocking"
                );
                assert_eq!(up.1, Transport::Udp);
                let after_unblock = up.0.as_secs_f64() - UNBLOCK;
                assert!(
                    (0.0..=3.5).contains(&after_unblock),
                    "recovered {after_unblock} s after unblocking"
                );
            }
        }
    }
}
