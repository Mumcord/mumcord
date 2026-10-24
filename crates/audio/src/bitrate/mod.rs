//! Adaptive Opus bitrate (FR-1.24): the highest bitrate the server allows,
//! lowered when packet loss rises and raised again when it recovers.
//!
//! Loss is answered mainly with in-band FEC (the expected-loss hint); the
//! bitrate is only trimmed gently and kept above a loss-dependent floor, so
//! moderate loss never drops voice quality to a few kbit/s. Recovery takes
//! seconds, not minutes.
//!
//! The server's maximum bandwidth covers whole packets, so the per-packet
//! overhead is subtracted using the same formula the official Mumble client
//! uses (`AudioInput::getNetworkBandwidth`): IP (20) + UDP (8) + crypto (4) +
//! voice header (1 + 2) + one byte per frame, +12 when tunnelled through TCP,
//! +12 with positional data, in bytes per packet.

/// Ceiling even when the server allows more: mono Opus voice gains next to
/// nothing above this, and it stays below typical server limits.
pub const MAX_BITRATE_BPS: u32 = 96_000;
/// Absolute floor, only for servers whose limit leaves less; below this Opus
/// voice becomes hard to understand.
pub const MIN_BITRATE_BPS: u32 = 8_000;

/// Share of the server's bandwidth limit kept free. The server (`Server::processMsg`,
/// `BandwidthRecord`) suppresses every packet that would push the average over its last 360
/// packets (7.2 s) above the limit. Opus VBR and sender/network jitter (a delayed batch of
/// packets shortens the measured window) overshoot by a few percent; without headroom the
/// server then drops whole runs of packets, heard as 200–600 ms gaps.
const SERVER_HEADROOM: f32 = 0.10;

/// Loss is not necessarily congestion, and Opus in-band FEC repairs random loss well as long
/// as it has bits to spend, so the bitrate only goes as low as the loss level demands. The
/// thresholds are generous because the measured loss is the larger of downlink loss and
/// *round-trip* ping loss (10 % each way reads as ~19 %), sampled from a handful of pings:
/// below [`HEAVY_LOSS`] it stays at or above [`MODERATE_FLOOR_BPS`]…
const MODERATE_FLOOR_BPS: u32 = 32_000;
/// …from here at or above [`HEAVY_FLOOR_BPS`]…
const HEAVY_LOSS: f32 = 0.25;
const HEAVY_FLOOR_BPS: u32 = 20_000;
/// …and from here it may fall to [`SEVERE_FLOOR_BPS`].
const SEVERE_LOSS: f32 = 0.40;
const SEVERE_FLOOR_BPS: u32 = 12_000;
/// Smoothed loss from which the bitrate is cut (by [`MODERATE_CUT`], or [`HEAVY_CUT`] from
/// [`HEAVY_LOSS`]), at most once per [`CUT_COOLDOWN_MS`].
const CUT_LOSS: f32 = 0.05;
const MODERATE_CUT: f32 = 0.85;
const HEAVY_CUT: f32 = 0.7;
const CUT_COOLDOWN_MS: u64 = 2_000;
/// Smoothed loss below which the link counts as recovered.
const GOOD_LOSS: f32 = 0.03;
/// The link must stay good this long before the first raise…
const RECOVERY_MS: u64 = 3_000;
/// …and between further raises; from the severe floor to a 50 kbit/s ceiling takes about
/// 7 raises, so a clean link is back at full quality ~10 s after the loss reading clears.
const RAISE_INTERVAL_MS: u64 = 1_000;
const RAISE_FACTOR: f32 = 1.25;
const MIN_RAISE_BPS: u32 = 4_000;
/// Round-trip time at which the link counts as congested…
const HIGH_RTT_MS: u32 = 500;
/// …and is treated like this much loss (moderate: a gentle cut down to the moderate floor).
const HIGH_RTT_LOSS: f32 = 0.1;
/// Weight of the newest loss sample in the moving average (one sample per second; the
/// transport already averages over a few seconds, but from only ~5 pings).
const LOSS_SMOOTHING: f32 = 0.3;
/// Extra expected loss reported to the encoder so FEC has some headroom.
const LOSS_HINT_MARGIN: u8 = 2;
const MAX_LOSS_HINT: u8 = 30;

/// How voice reaches the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Udp,
    TcpTunnel,
}

/// One measurement from the voice transport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkStats {
    /// Fraction of voice packets lost since the previous measurement, 0.0–1.0.
    pub loss_fraction: f32,
    pub rtt_ms: Option<u32>,
}

/// What the encoder should use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitrateTarget {
    pub bitrate_bps: u32,
    /// For `OpusEncoder::set_packet_loss_percent` (in-band FEC strength).
    pub loss_percent: u8,
}

/// Bits per second spent on headers for a packet carrying `frames` 10 ms frames.
pub fn network_overhead_bps(frames_per_packet: u32, transport: Transport, positional: bool) -> u32 {
    let frames = frames_per_packet.max(1);
    let mut bytes = 20 + 8 + 4 + 1 + 2 + frames;
    if transport == Transport::TcpTunnel {
        bytes += 12;
    }
    if positional {
        bytes += 12;
    }
    // bytes × 8 bit × (100 / frames) packets per second.
    bytes * 800 / frames
}

pub struct BitrateController {
    server_max_bps: Option<u32>,
    transport: Transport,
    positional: bool,
    frames_per_packet: u32,
    bitrate: u32,
    smoothed_loss: f32,
    last_cut_ms: Option<u64>,
    good_since_ms: Option<u64>,
    last_raise_ms: Option<u64>,
}

impl Default for BitrateController {
    fn default() -> Self {
        Self::new()
    }
}

impl BitrateController {
    /// Starts at the ceiling for 20 ms packets over UDP without a server limit.
    pub fn new() -> Self {
        Self {
            server_max_bps: None,
            transport: Transport::Udp,
            positional: false,
            frames_per_packet: 2,
            bitrate: MAX_BITRATE_BPS,
            smoothed_loss: 0.0,
            last_cut_ms: None,
            good_since_ms: None,
            last_raise_ms: None,
        }
    }

    /// The server's maximum bandwidth per user in bits per second (from
    /// ServerSync/ServerConfig); `None` or 0 means unlimited. Resets to the
    /// new ceiling, as this is learned on connect.
    pub fn set_server_max_bandwidth(&mut self, max_bps: Option<u32>) {
        self.server_max_bps = max_bps.filter(|&m| m > 0);
        self.bitrate = self.ceiling();
    }

    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
        self.clamp();
    }

    pub fn set_positional(&mut self, positional: bool) {
        self.positional = positional;
        self.clamp();
    }

    pub fn set_frames_per_packet(&mut self, frames: u32) {
        self.frames_per_packet = frames.max(1);
        self.clamp();
    }

    /// Highest bitrate the current server limit (minus headroom) and packet overhead allow.
    pub fn ceiling(&self) -> u32 {
        let overhead =
            network_overhead_bps(self.frames_per_packet, self.transport, self.positional);
        self.server_max_bps
            .map_or(MAX_BITRATE_BPS, |max| {
                let usable = (max as f32 * (1.0 - SERVER_HEADROOM)) as u32;
                usable.saturating_sub(overhead)
            })
            .clamp(MIN_BITRATE_BPS, MAX_BITRATE_BPS)
    }

    /// Lowest bitrate the current (smoothed) loss justifies.
    fn floor(&self) -> u32 {
        let floor = if self.smoothed_loss >= SEVERE_LOSS {
            SEVERE_FLOOR_BPS
        } else if self.smoothed_loss >= HEAVY_LOSS {
            HEAVY_FLOOR_BPS
        } else {
            MODERATE_FLOOR_BPS
        };
        floor.min(self.ceiling())
    }

    /// Feeds a measurement taken at `now_ms` (monotonic) and returns the new target.
    pub fn update(&mut self, stats: LinkStats, now_ms: u64) -> BitrateTarget {
        let mut loss = stats.loss_fraction.clamp(0.0, 1.0);
        if stats.rtt_ms.is_some_and(|rtt| rtt >= HIGH_RTT_MS) {
            loss = loss.max(HIGH_RTT_LOSS);
        }
        self.smoothed_loss += (loss - self.smoothed_loss) * LOSS_SMOOTHING;

        let since = |t: Option<u64>| t.map_or(u64::MAX, |t| now_ms.saturating_sub(t));
        if self.smoothed_loss >= CUT_LOSS {
            self.good_since_ms = None;
            if since(self.last_cut_ms) >= CUT_COOLDOWN_MS && self.bitrate > self.floor() {
                let cut = if self.smoothed_loss >= HEAVY_LOSS {
                    HEAVY_CUT
                } else {
                    MODERATE_CUT
                };
                self.bitrate = (self.bitrate as f32 * cut) as u32;
                self.last_cut_ms = Some(now_ms);
            }
        } else if self.smoothed_loss < GOOD_LOSS {
            let good_since = *self.good_since_ms.get_or_insert(now_ms);
            let recovered = now_ms.saturating_sub(good_since) >= RECOVERY_MS;
            if recovered
                && since(self.last_raise_ms) >= RAISE_INTERVAL_MS
                && self.bitrate < self.ceiling()
            {
                let raised = (self.bitrate as f32 * RAISE_FACTOR) as u32;
                self.bitrate = raised.max(self.bitrate + MIN_RAISE_BPS);
                self.last_raise_ms = Some(now_ms);
            }
        } else {
            self.good_since_ms = None;
        }
        // Loss easing off (e.g. severe → moderate) lifts the floor straight away.
        self.bitrate = self.bitrate.max(self.floor());
        self.clamp();
        self.target()
    }

    pub fn target(&self) -> BitrateTarget {
        let loss_percent = if self.smoothed_loss < 0.01 {
            0
        } else {
            ((self.smoothed_loss * 100.0).ceil() as u8 + LOSS_HINT_MARGIN).min(MAX_LOSS_HINT)
        };
        BitrateTarget {
            bitrate_bps: self.bitrate,
            loss_percent,
        }
    }

    fn clamp(&mut self) {
        self.bitrate = self.bitrate.clamp(MIN_BITRATE_BPS, self.ceiling());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(loss: f32) -> LinkStats {
        LinkStats {
            loss_fraction: loss,
            rtt_ms: Some(30),
        }
    }

    /// Feeds one measurement per second for `secs` seconds starting at `start_s`.
    fn feed(c: &mut BitrateController, loss: f32, start_s: u64, secs: u64) -> BitrateTarget {
        (start_s..start_s + secs)
            .map(|s| c.update(stats(loss), s * 1000))
            .last()
            .expect("at least one second")
    }

    /// Ceiling for a 72 kbit/s server with 20 ms packets over UDP.
    const CEILING_72K: u32 = 64_800 - 14_800;

    #[test]
    fn overhead_matches_mumble_formula() {
        // 20 ms over UDP: (35 + 2) bytes × 50 packets × 8 bit.
        assert_eq!(network_overhead_bps(2, Transport::Udp, false), 14_800);
        assert_eq!(network_overhead_bps(1, Transport::Udp, false), 28_800);
        assert_eq!(network_overhead_bps(2, Transport::TcpTunnel, true), 24_400);
    }

    #[test]
    fn starts_at_ceiling_and_respects_server_max() {
        let mut c = BitrateController::new();
        assert_eq!(c.target().bitrate_bps, MAX_BITRATE_BPS);

        // 10 % of the server limit stays free for VBR and jitter.
        c.set_server_max_bandwidth(Some(72_000));
        assert_eq!(c.target().bitrate_bps, CEILING_72K);
        c.set_transport(Transport::TcpTunnel);
        assert_eq!(c.target().bitrate_bps, 64_800 - 19_600);

        c.set_server_max_bandwidth(Some(558_000));
        assert_eq!(c.target().bitrate_bps, MAX_BITRATE_BPS);
        c.set_server_max_bandwidth(Some(10_000));
        assert_eq!(c.target().bitrate_bps, MIN_BITRATE_BPS);
        c.set_server_max_bandwidth(Some(0));
        assert_eq!(c.target().bitrate_bps, MAX_BITRATE_BPS);
    }

    #[test]
    fn clean_link_stays_at_ceiling() {
        let mut c = BitrateController::new();
        c.set_server_max_bandwidth(Some(60_000));
        let target = feed(&mut c, 0.0, 0, 60);
        assert_eq!(target.bitrate_bps, 54_000 - 14_800);
        assert_eq!(target.loss_percent, 0);
    }

    #[test]
    fn moderate_loss_keeps_quality_and_uses_fec() {
        for server in [None, Some(72_000)] {
            let mut c = BitrateController::new();
            c.set_server_max_bandwidth(server);
            for s in 0..120 {
                let t = c.update(stats(0.10), s * 1000);
                assert!(t.bitrate_bps >= 24_000, "{server:?} at {s} s: {t:?}");
            }
            let t = c.target();
            assert!(t.bitrate_bps < c.ceiling(), "trimmed a little: {t:?}");
            assert!((10..=MAX_LOSS_HINT).contains(&t.loss_percent), "{t:?}");
        }
    }

    #[test]
    fn noisy_round_trip_loss_of_ten_percent_each_way_stays_moderate() {
        // ~19 % round-trip ping loss, read from a few pings: 0 %, 25 % or 50 %.
        let pattern = [0.25, 0.0, 0.25, 0.25, 0.0, 0.5, 0.25, 0.0, 0.25, 0.0];
        let mut c = BitrateController::new();
        c.set_server_max_bandwidth(Some(72_000));
        for s in 0..200u64 {
            let t = c.update(stats(pattern[s as usize % pattern.len()]), s * 1000);
            assert!(t.bitrate_bps >= 24_000, "at {s} s: {t:?}");
        }
    }

    #[test]
    fn heavy_and_severe_loss_may_go_lower() {
        let mut c = BitrateController::new();
        let heavy = feed(&mut c, 0.30, 0, 60);
        assert!(heavy.bitrate_bps < 24_000, "{heavy:?}");
        assert_eq!(heavy.bitrate_bps, HEAVY_FLOOR_BPS);
        assert_eq!(heavy.loss_percent, MAX_LOSS_HINT);

        let severe = feed(&mut c, 0.6, 60, 60);
        assert_eq!(severe.bitrate_bps, SEVERE_FLOOR_BPS);
        assert_eq!(severe.loss_percent, MAX_LOSS_HINT);

        // Loss easing to moderate lifts the floor again.
        let eased = feed(&mut c, 0.10, 120, 30);
        assert!(eased.bitrate_bps >= MODERATE_FLOOR_BPS, "{eased:?}");
    }

    #[test]
    fn recovers_within_twenty_seconds() {
        for server in [None, Some(72_000)] {
            let mut c = BitrateController::new();
            c.set_server_max_bandwidth(server);
            let low = feed(&mut c, 0.6, 0, 60);
            assert_eq!(low.bitrate_bps, SEVERE_FLOOR_BPS);

            let mut prev = low;
            let mut full_at = None;
            for s in 60..200 {
                let next = c.update(stats(0.0), s * 1000);
                assert!(
                    next.bitrate_bps >= prev.bitrate_bps,
                    "never drops on a clean link"
                );
                if full_at.is_none() && next.bitrate_bps == c.ceiling() {
                    full_at = Some(s - 60);
                }
                prev = next;
            }
            let full_at = full_at.expect("recovers");
            assert!(full_at <= 20, "{server:?}: full bitrate after {full_at} s");
            assert_eq!(prev.loss_percent, 0);
        }
    }

    #[test]
    fn cuts_have_a_cooldown() {
        let mut c = BitrateController::new();
        c.update(stats(1.0), 0);
        assert_eq!(c.target().bitrate_bps, (96_000.0 * HEAVY_CUT) as u32);
        // Within the cooldown nothing changes.
        c.update(stats(1.0), 1_000);
        assert_eq!(c.target().bitrate_bps, (96_000.0 * HEAVY_CUT) as u32);
        c.update(stats(1.0), 2_000);
        assert!(c.target().bitrate_bps < (96_000.0 * HEAVY_CUT) as u32);
    }

    #[test]
    fn single_loss_blip_is_smoothed_away() {
        let mut c = BitrateController::new();
        c.update(stats(0.06), 0);
        assert_eq!(c.target().bitrate_bps, MAX_BITRATE_BPS);
    }

    #[test]
    fn high_rtt_counts_as_moderate_congestion() {
        let mut c = BitrateController::new();
        for s in 0..60 {
            c.update(
                LinkStats {
                    loss_fraction: 0.0,
                    rtt_ms: Some(800),
                },
                s * 1000,
            );
        }
        let t = c.target();
        assert!(t.bitrate_bps < MAX_BITRATE_BPS, "{t:?}");
        assert!(t.bitrate_bps >= MODERATE_FLOOR_BPS, "{t:?}");
    }

    #[test]
    fn never_exceeds_server_ceiling() {
        for max in [40_000, 72_000, 130_000] {
            let mut c = BitrateController::new();
            c.set_server_max_bandwidth(Some(max));
            let ceiling = c.ceiling();
            assert!(ceiling + 14_800 <= max);
            for (loss, start) in [(0.3, 0), (0.1, 30), (0.0, 60), (0.6, 120), (0.0, 150)] {
                for s in start..start + 30 {
                    let t = c.update(stats(loss), s * 1000);
                    assert!(t.bitrate_bps <= ceiling, "{max}: {t:?}");
                }
            }
            assert_eq!(c.target().bitrate_bps, ceiling);
        }
        // A floor never lifts the bitrate over a small server limit.
        let mut c = BitrateController::new();
        c.set_server_max_bandwidth(Some(40_000));
        let t = feed(&mut c, 0.1, 0, 60);
        assert_eq!(t.bitrate_bps, 36_000 - 14_800);
    }
}
