//! `clock` (FR-6.21): wall time rounded down to the second plus the local UTC offset, so a
//! plugin can find "today" but cannot time anything precisely.

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::clock::{Datetime, Host, add_to_linker};
use crate::runtime::{HostApi, PluginState};

pub(crate) struct ClockApi;

impl HostApi for ClockApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn now(&mut self) -> Datetime {
        now()
    }
}

fn now() -> Datetime {
    let zoned = jiff::ZonedDateTime::now();
    Datetime {
        // A clock set before 1970 has no seconds to report.
        unix_seconds: u64::try_from(zoned.timestamp().as_second()).unwrap_or(0),
        utc_offset_minutes: zoned.offset().seconds() / 60,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn whole_seconds_and_a_plausible_offset() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test value");
        let time = now();
        assert!(time.unix_seconds >= before.as_secs());
        assert!(time.unix_seconds <= before.as_secs() + 1);
        assert!((-14 * 60..=14 * 60).contains(&time.utc_offset_minutes));
    }
}
