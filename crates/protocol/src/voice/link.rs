//! Transport choice (UDP or TCP tunnel) and link statistics, as pure logic over a clock passed
//! in by the caller so it can be tested without sockets.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::crypt::CryptStats;

/// Interval of the encrypted UDP connectivity pings.
pub(crate) const PING_INTERVAL: Duration = Duration::from_secs(1);
/// Without any valid UDP packet for this long, voice moves to the TCP tunnel.
const UDP_TIMEOUT: Duration = Duration::from_secs(4);
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
}

pub(crate) struct Link {
    transport: Transport,
    ever_udp: bool,
    last_udp: Option<Instant>,
    /// Timestamp of the ping still waiting for a reply.
    outstanding: Option<u64>,
    replies_in_row: u32,
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

    /// Periodic check; returns the new transport if UDP stopped working.
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
            .is_none_or(|t| now.duration_since(t) > UDP_TIMEOUT);
        (self.transport == Transport::Udp && silent).then(|| self.switch(Transport::Tcp))
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
        }
    }

    fn switch(&mut self, to: Transport) -> Transport {
        self.transport = to;
        self.replies_in_row = 0;
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
    fn falls_back_after_silence_and_needs_three_replies_to_recover() {
        let t0 = Instant::now();
        let mut l = Link::new(t0);
        l.ping_sent(1);
        l.udp_received(t0);
        l.ping_reply(1, MS);
        assert_eq!(l.tick(t0 + Duration::from_secs(3), stats(1, 0)), None);
        assert_eq!(
            l.tick(t0 + Duration::from_secs(5), stats(1, 0)),
            Some(Transport::Tcp)
        );
        assert_eq!(l.stats(stats(1, 0)).rtt, None);

        let t = t0 + Duration::from_secs(6);
        for ts in 2..4 {
            l.ping_sent(ts);
            l.udp_received(t);
            assert_eq!(l.ping_reply(ts, MS), None);
        }
        // A missed ping resets the run.
        l.ping_sent(4);
        l.ping_sent(5);
        assert_eq!(l.ping_reply(4, MS), None, "stale reply ignored");
        assert_eq!(l.ping_reply(5, MS), None);
        l.ping_sent(6);
        assert_eq!(l.ping_reply(6, MS), None);
        l.ping_sent(7);
        assert_eq!(l.ping_reply(7, MS), Some(Transport::Udp));
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
}
