//! The Mumcord hello another user sends as plugin data.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = mumcord_engine::mumcord::decode(data);
});
