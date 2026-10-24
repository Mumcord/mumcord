//! Pure connection logic: the state machine of spec §7 with reconnect backoff, the
//! trust-on-first-use decision and the self mute/deafen rules. No I/O, so it is unit-tested.

use std::time::Duration;

use mumcord_store::ServerId;

use super::types::{ConnState, EngineError, ErrorKind};

/// Failed reconnect attempts before giving up (FR-1.13).
pub const MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Wait before reconnect attempt `attempt` (1-based): quick first retries for short blips,
/// then capped so a returning server is found within half a minute.
pub fn backoff(attempt: u32) -> Duration {
    const STEPS: [u64; 5] = [1, 2, 4, 8, 15];
    let secs = STEPS
        .get(attempt.saturating_sub(1) as usize)
        .copied()
        .unwrap_or(30);
    Duration::from_secs(secs)
}

/// How a connection attempt failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Network trouble (unreachable, timeout, closed): worth retrying while reconnecting.
    Transient(EngineError),
    /// The server said no (password, name, certificate): retrying cannot help.
    Fatal(EngineError),
}

/// What the supervisor does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Retry(Duration),
    Stop,
}

/// The connection state machine. Every method returns the step to take; the new state is
/// in [`Machine::state`].
#[derive(Debug, Clone)]
pub struct Machine {
    state: ConnState,
    attempt: u32,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            state: ConnState::Disconnected { error: None },
            attempt: 0,
        }
    }
}

impl Machine {
    pub fn state(&self) -> &ConnState {
        &self.state
    }

    fn server_id(&self) -> Option<ServerId> {
        self.state.server_id()
    }

    pub fn connect(&mut self, server_id: ServerId) {
        self.attempt = 0;
        self.state = ConnState::Connecting { server_id };
    }

    /// ServerSync received.
    pub fn synced(&mut self) {
        if let Some(server_id) = self.server_id() {
            self.attempt = 0;
            self.state = ConnState::Connected { server_id };
        }
    }

    /// An established connection dropped unexpectedly.
    pub fn lost(&mut self) -> Step {
        self.attempt = 0;
        self.schedule_retry()
    }

    /// A connection attempt failed.
    pub fn failed(&mut self, failure: Failure) -> Step {
        match (&self.state, failure) {
            (ConnState::Reconnecting { .. }, Failure::Transient(_)) => self.schedule_retry(),
            (_, Failure::Transient(error) | Failure::Fatal(error)) => self.stop(Some(error)),
        }
    }

    /// The user disconnected or cancelled reconnecting.
    pub fn cancel(&mut self) {
        self.stop(None);
    }

    /// The connection ended for good (e.g. kicked).
    pub fn end(&mut self, error: Option<EngineError>) {
        self.stop(error);
    }

    fn schedule_retry(&mut self) -> Step {
        let Some(server_id) = self.server_id() else {
            return Step::Stop;
        };
        self.attempt += 1;
        if self.attempt > MAX_RECONNECT_ATTEMPTS {
            return self.stop(Some(EngineError::new(
                ErrorKind::ServerUnreachable,
                format!("gave up after {MAX_RECONNECT_ATTEMPTS} reconnect attempts"),
            )));
        }
        let delay = backoff(self.attempt);
        self.state = ConnState::Reconnecting {
            server_id,
            attempt: self.attempt,
            max_attempts: MAX_RECONNECT_ATTEMPTS,
            next_retry_secs: delay.as_secs() as u32,
        };
        Step::Retry(delay)
    }

    fn stop(&mut self, error: Option<EngineError>) -> Step {
        self.attempt = 0;
        self.state = ConnState::Disconnected { error };
        Step::Stop
    }
}

/// Outcome of comparing the presented server certificate with the stored decision (FR-1.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertCheck {
    Trusted,
    /// First connection: ask the user.
    Unknown,
    /// A trusted server presented another certificate: blocking warning.
    Changed {
        previous: String,
    },
}

pub fn check_cert(stored: Option<&str>, presented: &str) -> CertCheck {
    match stored {
        Some(s) if s.eq_ignore_ascii_case(presented) => CertCheck::Trusted,
        Some(s) => CertCheck::Changed {
            previous: s.to_owned(),
        },
        None => CertCheck::Unknown,
    }
}

/// SHA-256 fingerprint as shown to users: upper-case hex pairs separated by colons.
pub fn format_fingerprint(sha256_hex: &str) -> String {
    sha256_hex
        .as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

/// Own mute/deafen state (FR-1.22): deafen implies mute, undeafen restores the previous mute
/// state, and unmuting while deafened also undeafens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelfVoice {
    pub mute: bool,
    pub deaf: bool,
    mute_before_deaf: bool,
}

impl SelfVoice {
    pub fn set_mute(&mut self, mute: bool) {
        if !mute && self.deaf {
            self.deaf = false;
        }
        self.mute = mute;
    }

    pub fn set_deaf(&mut self, deaf: bool) {
        if deaf == self.deaf {
            return;
        }
        if deaf {
            self.mute_before_deaf = self.mute;
            self.mute = true;
        } else {
            self.mute = self.mute_before_deaf;
        }
        self.deaf = deaf;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreachable() -> Failure {
        Failure::Transient(EngineError::new(ErrorKind::ServerUnreachable, "refused"))
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        let secs: Vec<u64> = (1..=MAX_RECONNECT_ATTEMPTS)
            .map(|a| backoff(a).as_secs())
            .collect();
        assert_eq!(secs, [1, 2, 4, 8, 15, 30, 30, 30, 30, 30]);
    }

    #[test]
    fn connect_sync_and_user_disconnect() {
        let mut m = Machine::default();
        m.connect(7);
        assert_eq!(m.state(), &ConnState::Connecting { server_id: 7 });
        m.synced();
        assert_eq!(m.state(), &ConnState::Connected { server_id: 7 });
        m.cancel();
        assert_eq!(m.state(), &ConnState::Disconnected { error: None });
    }

    #[test]
    fn first_attempt_failure_does_not_retry() {
        let mut m = Machine::default();
        m.connect(1);
        assert_eq!(m.failed(unreachable()), Step::Stop);
        assert!(matches!(
            m.state(),
            ConnState::Disconnected { error: Some(e) } if e.kind == ErrorKind::ServerUnreachable
        ));
    }

    #[test]
    fn lost_connection_retries_with_backoff_and_recovers() {
        let mut m = Machine::default();
        m.connect(1);
        m.synced();
        assert_eq!(m.lost(), Step::Retry(Duration::from_secs(1)));
        assert_eq!(
            m.state(),
            &ConnState::Reconnecting {
                server_id: 1,
                attempt: 1,
                max_attempts: MAX_RECONNECT_ATTEMPTS,
                next_retry_secs: 1
            }
        );
        assert_eq!(m.failed(unreachable()), Step::Retry(Duration::from_secs(2)));
        m.synced();
        assert_eq!(m.state(), &ConnState::Connected { server_id: 1 });
        // A later loss starts counting from 1 again.
        assert_eq!(m.lost(), Step::Retry(Duration::from_secs(1)));
    }

    #[test]
    fn gives_up_after_max_attempts() {
        let mut m = Machine::default();
        m.connect(1);
        m.synced();
        let mut step = m.lost();
        let mut retries = 1;
        while let Step::Retry(_) = step {
            step = m.failed(unreachable());
            retries += 1;
        }
        assert_eq!(retries, MAX_RECONNECT_ATTEMPTS + 1);
        assert!(matches!(
            m.state(),
            ConnState::Disconnected { error: Some(e) } if e.message.contains("gave up")
        ));
    }

    #[test]
    fn fatal_failure_while_reconnecting_stops() {
        let mut m = Machine::default();
        m.connect(1);
        m.synced();
        m.lost();
        let wrong = EngineError::new(ErrorKind::WrongPassword, "rejected");
        assert_eq!(m.failed(Failure::Fatal(wrong.clone())), Step::Stop);
        assert_eq!(m.state(), &ConnState::Disconnected { error: Some(wrong) });
    }

    #[test]
    fn cancel_while_reconnecting() {
        let mut m = Machine::default();
        m.connect(1);
        m.synced();
        m.lost();
        m.cancel();
        assert_eq!(m.state(), &ConnState::Disconnected { error: None });
        // A stale sync after cancelling must not resurrect the connection.
        m.synced();
        assert_eq!(m.state(), &ConnState::Disconnected { error: None });
    }

    #[test]
    fn tofu_decisions() {
        assert_eq!(check_cert(None, "AA:BB"), CertCheck::Unknown);
        assert_eq!(check_cert(Some("aa:bb"), "AA:BB"), CertCheck::Trusted);
        assert_eq!(
            check_cert(Some("AA:BB"), "CC:DD"),
            CertCheck::Changed {
                previous: "AA:BB".into()
            }
        );
    }

    #[test]
    fn fingerprint_format() {
        assert_eq!(format_fingerprint("0a1bff"), "0A:1B:FF");
    }

    #[test]
    fn deafen_implies_mute_and_restores() {
        let mut v = SelfVoice::default();
        v.set_deaf(true);
        assert!(v.mute && v.deaf);
        v.set_deaf(false);
        assert!(!v.mute && !v.deaf);

        v.set_mute(true);
        v.set_deaf(true);
        v.set_deaf(false);
        assert!(v.mute && !v.deaf, "previous mute state restored");
    }

    #[test]
    fn unmute_while_deafened_undeafens() {
        let mut v = SelfVoice::default();
        v.set_deaf(true);
        v.set_mute(false);
        assert!(!v.mute && !v.deaf);
    }
}
