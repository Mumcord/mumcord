//! Adaptive Opus bitrate (FR-1.24): the highest bitrate the server allows,
//! lowered when packet loss rises and raised again when it recovers.
//!
//! The server's maximum bandwidth covers whole packets, so the per-packet
//! overhead is subtracted using the same formula the official Mumble client
//! uses (`AudioInput::getNetworkBandwidth`): IP (20) + UDP (8) + crypto (4) +
//! voice header (1 + 2) + one byte per frame, +12 when tunnelled through TCP,
//! +12 with positional data, in bytes per packet.

/// Ceiling even when the server allows more: mono Opus voice gains next to
/// nothing above this, and it stays below typical server limits.
pub const MAX_BITRATE_BPS: u32 = 96_000;
/// Floor; below this Opus voice becomes hard to understand.
pub const MIN_BITRATE_BPS: u32 = 8_000;

/// Smoothed loss at which the bitrate is cut by [`MODERATE_CUT`]…
const MODERATE_LOSS: f32 = 0.05;
const MODERATE_CUT: f32 = 0.8;
/// …and at which it is cut harder by [`HEAVY_CUT`].
const HEAVY_LOSS: f32 = 0.15;
const HEAVY_CUT: f32 = 0.6;
/// Smoothed loss below which the link counts as recovered.
const GOOD_LOSS: f32 = 0.02;
/// Minimum time between two cuts, so one loss burst is not punished twice.
const CUT_COOLDOWN_MS: u64 = 2_000;
/// The link must stay good this long before the first raise…
const RECOVERY_MS: u64 = 10_000;
/// …and between further raises.
const RAISE_INTERVAL_MS: u64 = 5_000;
const RAISE_FACTOR: f32 = 1.1;
const MIN_RAISE_BPS: u32 = 4_000;
/// Round-trip time at which the link counts as congested…
const HIGH_RTT_MS: u32 = 500;
/// …and is treated like this much loss (a moderate cut after two samples).
const HIGH_RTT_LOSS: f32 = 0.1;
/// Weight of the newest loss sample in the moving average.
const LOSS_SMOOTHING: f32 = 0.5;
/// Extra expected loss reported to the encoder so FEC has some headroom.
const LOSS_HINT_MARGIN: u8 = 2;
const MAX_LOSS_HINT: u8 = 25;

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

    /// Highest bitrate the current server limit and packet overhead allow.
    pub fn ceiling(&self) -> u32 {
        let overhead =
            network_overhead_bps(self.frames_per_packet, self.transport, self.positional);
        self.server_max_bps
            .map_or(MAX_BITRATE_BPS, |max| max.saturating_sub(overhead))
            .clamp(MIN_BITRATE_BPS, MAX_BITRATE_BPS)
    }

    /// Feeds a measurement taken at `now_ms` (monotonic) and returns the new target.
    pub fn update(&mut self, stats: LinkStats, now_ms: u64) -> BitrateTarget {
        let mut loss = stats.loss_fraction.clamp(0.0, 1.0);
        if stats.rtt_ms.is_some_and(|rtt| rtt >= HIGH_RTT_MS) {
            loss = loss.max(HIGH_RTT_LOSS);
        }
        self.smoothed_loss += (loss - self.smoothed_loss) * LOSS_SMOOTHING;

        let since = |t: Option<u64>| t.map_or(u64::MAX, |t| now_ms.saturating_sub(t));
        if self.smoothed_loss >= MODERATE_LOSS {
            self.good_since_ms = None;
            if since(self.last_cut_ms) >= CUT_COOLDOWN_MS {
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
            let raise_due = since(self.last_raise_ms) >= RAISE_INTERVAL_MS
                && since(self.last_cut_ms) >= RECOVERY_MS;
            if recovered && raise_due && self.bitrate < self.ceiling() {
                let raised = (self.bitrate as f32 * RAISE_FACTOR) as u32;
                self.bitrate = raised.max(self.bitrate + MIN_RAISE_BPS);
                self.last_raise_ms = Some(now_ms);
            }
        } else {
            self.good_since_ms = None;
        }
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

        c.set_server_max_bandwidth(Some(72_000));
        assert_eq!(c.target().bitrate_bps, 72_000 - 14_800);
        c.set_transport(Transport::TcpTunnel);
        assert_eq!(c.target().bitrate_bps, 72_000 - 19_600);

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
        assert_eq!(target.bitrate_bps, 60_000 - 14_800);
        assert_eq!(target.loss_percent, 0);
    }

    #[test]
    fn loss_lowers_and_recovery_restores() {
        let mut c = BitrateController::new();
        let lossy = feed(&mut c, 0.10, 0, 10);
        assert!(lossy.bitrate_bps < 50_000, "{lossy:?}");
        assert!(lossy.bitrate_bps >= MIN_BITRATE_BPS);
        assert!(
            (10..=MAX_LOSS_HINT).contains(&lossy.loss_percent),
            "{lossy:?}"
        );

        let mut restored = c.target();
        for s in 10..120 {
            let next = c.update(stats(0.0), s * 1000);
            assert!(
                next.bitrate_bps >= restored.bitrate_bps,
                "never drops on a clean link"
            );
            restored = next;
        }
        assert_eq!(restored.bitrate_bps, MAX_BITRATE_BPS);
        assert_eq!(restored.loss_percent, 0);
    }

    #[test]
    fn heavy_loss_cuts_harder_with_cooldown() {
        let mut c = BitrateController::new();
        c.update(stats(0.4), 0);
        assert_eq!(c.target().bitrate_bps, (96_000.0 * HEAVY_CUT) as u32);
        // Within the cooldown nothing changes.
        c.update(stats(0.4), 1_000);
        assert_eq!(c.target().bitrate_bps, (96_000.0 * HEAVY_CUT) as u32);
        c.update(stats(0.4), 2_000);
        assert!(c.target().bitrate_bps < (96_000.0 * HEAVY_CUT) as u32);
    }

    #[test]
    fn single_loss_blip_is_smoothed_away() {
        let mut c = BitrateController::new();
        c.update(stats(0.06), 0);
        assert_eq!(c.target().bitrate_bps, MAX_BITRATE_BPS);
    }

    #[test]
    fn high_rtt_counts_as_congestion() {
        let mut c = BitrateController::new();
        for s in 0..5 {
            c.update(
                LinkStats {
                    loss_fraction: 0.0,
                    rtt_ms: Some(800),
                },
                s * 1000,
            );
        }
        assert!(c.target().bitrate_bps < MAX_BITRATE_BPS);
    }

    #[test]
    fn recovery_never_exceeds_server_ceiling() {
        let mut c = BitrateController::new();
        c.set_server_max_bandwidth(Some(40_000));
        feed(&mut c, 0.3, 0, 6);
        let target = feed(&mut c, 0.0, 6, 200);
        assert_eq!(target.bitrate_bps, 40_000 - 14_800);
    }
}
