//! `clock` (FR-6.21): wall time rounded down to the second plus the local UTC offset, so a
//! plugin can find "today" but cannot time anything precisely.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, Offset};
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
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let offset_seconds = Local::now().offset().fix().local_minus_utc();
    Datetime {
        unix_seconds,
        utc_offset_minutes: offset_seconds / 60,
    }
}

#[cfg(test)]
mod tests {
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
