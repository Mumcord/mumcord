//! A decrypted UDP or tunnelled voice packet, in the legacy and the protobuf format.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    mumble_client_protocol::voice::fuzz_decode(data);
});
