//! TCP control channel bytes as a server sends them: decodes frames until the buffer is used up
//! or a frame is incomplete or rejected.
#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use mumcord_protocol::framing;

fuzz_target!(|data: &[u8]| {
    let mut buf = BytesMut::from(data);
    while let Ok(Some(frame)) = framing::decode(&mut buf) {
        let _ = frame.message_type();
    }
});
