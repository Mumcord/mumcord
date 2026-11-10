//! The endless `.media` document of a media link (FR-7.89, FR-7.94).
//!
//! A new-format media link carries one Bovnar document that never ends:
//!
//! ```bovnar
//! #!bovnar 1.2
//! .media = \x00 …messages… \x00;
//! ```
//!
//! A message is `varint(channel) varint(length) bytes`. Every message starts a new chunk; a
//! message above 64 KiB spans several chunks, each starting with its channel varint.
//! [`MediaWriter`] writes the opening once, then messages, then the terminator when the link
//! closes. [`MediaReader`] is fed bytes in pieces of any size and hands out each complete
//! message as soon as its last byte arrives, with the payload borrowed from the caller's
//! buffer whenever the message lies whole in it.
//!
//! The chunking, the varints and their bounds are `bovnar::codec`'s ([`MuxWriter`],
//! [`MuxReader`], [`Peek`]); this module adds the media profile: the fixed key, the channel and
//! length checks of [`super::check_message`] and the end rule of FR-7.94. A stream that ends at
//! a message boundary is a clean stop, not a truncated document; one that ends inside a message
//! is [`CodecError::Truncated`].
//!
//! Everything read is untrusted. A message on a channel outside the profile or over its
//! channel's cap is skipped before any of its payload is buffered, and every error names a
//! profile rule only — never received content.

use bovnar::ErrorCode;
use bovnar::codec::{Item, MuxReader, MuxWriter, Peek};

use super::{MAX_VIDEO_MESSAGE, MediaError, check_message};

/// The key of the media document's octet stream (FR-7.89).
const MEDIA_KEY: &str = "media";
/// The text before the stream's assignment: the Bovnar version of the document (FR-7.89).
const MEDIA_PREAMBLE: &[u8] = b"#!bovnar 1.2\n";

/// Why reading a media stream stopped, or why one message was dropped.
///
/// [`Dropped`](Self::Dropped) is recoverable: the reader skipped the message and the next
/// [`MediaReader::read`] goes on with the stream. The other variants latch: every later call
/// returns the first of them until [`MediaReader::reset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// A message outside the media profile; it was skipped without its payload being buffered.
    #[error("media message dropped: {0}")]
    Dropped(MediaError),
    /// The document framing is malformed, so the stream cannot go on.
    #[error("malformed media document: {0:?}")]
    Malformed(Option<ErrorCode>),
    /// The input ended inside a message, not at a message boundary.
    #[error("media stream ended inside a message")]
    Truncated,
}

/// One complete message of a media stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaMessage<'a> {
    /// The channel the message travelled on: a [`super::ChannelUse`] of the profile.
    pub channel: u16,
    /// Its bytes, borrowed from the caller's input when it lay whole in it, else from the
    /// reader's buffer until the next call.
    pub payload: &'a [u8],
}

/// Writes the endless `.media` document of a media link: [`begin`](Self::begin) once when the
/// link opens, any number of [`write_message`](Self::write_message) calls, and
/// [`finish`](Self::finish) when the link closes.
///
/// The writer keeps no per-stream state, so one value writes any number of streams: after
/// `finish`, the next `begin` opens the next document with nothing to reset.
#[derive(Debug)]
pub struct MediaWriter {
    mux: MuxWriter,
}

impl MediaWriter {
    /// A writer for the fixed media document. The opener is part of the profile, so this
    /// cannot fail.
    pub fn new() -> Self {
        let mux = MuxWriter::with_preamble(MEDIA_PREAMBLE, MEDIA_KEY)
            .expect("the fixed media document opening is valid");
        Self { mux }
    }

    /// The bytes that open the document: `#!bovnar 1.2`, the `.media` assignment and the NUL
    /// that starts the octet stream. Write them once per stream, with [`begin`](Self::begin).
    pub fn opening(&self) -> &[u8] {
        self.mux.opener()
    }

    /// The bytes that close the document: the octet stream's `0x00` and the `;`. Write them
    /// once per stream, with [`finish`](Self::finish).
    pub fn terminator(&self) -> &'static [u8] {
        self.mux.terminator()
    }

    /// Appends [`opening`](Self::opening) to `out`.
    pub fn begin(&self, out: &mut Vec<u8>) {
        self.mux.begin(out);
    }

    /// Appends one message to `out`: its channel, its length and `payload`, in the chunks of
    /// FR-7.89. Refuses, without writing anything, a channel outside the profile and a payload
    /// over the channel's cap (FR-7.94).
    pub fn write_message(
        &self,
        out: &mut Vec<u8>,
        channel: u16,
        payload: &[u8],
    ) -> Result<(), MediaError> {
        check_message(channel, payload.len())?;
        self.mux.write_message(out, u64::from(channel), payload);
        Ok(())
    }

    /// Appends [`terminator`](Self::terminator) to `out`; the link closes after it.
    pub fn finish(&self, out: &mut Vec<u8>) {
        self.mux.finish(out);
    }
}

impl Default for MediaWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads the endless `.media` document of a media link, fed bytes in pieces of any size.
///
/// [`read`](Self::read) returns each complete message as soon as it arrives, in the order the
/// messages completed. A split inside a chunk header, a channel or length varint or a payload
/// is fine. A message outside the profile is skipped and reported as
/// [`CodecError::Dropped`], and the stream goes on.
///
/// Call [`finish`](Self::finish) when the input ends. A stream that ends at a message boundary
/// is a clean stop even without its terminator; one that ends inside a message is
/// [`CodecError::Truncated`].
#[derive(Debug)]
pub struct MediaReader {
    mux: MuxReader,
    /// The channel of the message most recently peeked at, validated before it was delivered.
    channel: u16,
    /// A checked message's bytes are being delivered; more input may be needed to finish it,
    /// and no profile check may run before it does.
    delivering: bool,
    /// The first error that stopped the reader, so a later [`finish`](Self::finish) reports the
    /// real cause instead of calling a malformed stream truncated.
    fatal: Option<CodecError>,
}

impl MediaReader {
    /// A reader for the media profile: messages of at most [`MAX_VIDEO_MESSAGE`] bytes, the
    /// document under the `.media` key.
    pub fn new() -> Self {
        let mux = MuxReader::with_options(MAX_VIDEO_MESSAGE as u64, Some(MEDIA_KEY))
            .expect("the fixed media document key and limits are valid");
        Self {
            mux,
            channel: 0,
            delivering: false,
            fatal: None,
        }
    }

    /// Reads from the front of `input` and returns the next complete message, or `None` once
    /// every byte of `input` has been taken without completing one.
    ///
    /// A call that returns a message may leave bytes; pass the same slice to the next call
    /// until it returns `None`. After [`CodecError::Dropped`] the message was skipped whole;
    /// pass the rest of the same input to go on. A message is validated against the profile
    /// before its payload is exposed.
    pub fn read<'s, 'a: 's>(
        &'s mut self,
        input: &mut &'a [u8],
    ) -> Result<Option<MediaMessage<'s>>, CodecError> {
        if self.delivering {
            return self.deliver(input);
        }
        match self.prepare(input)? {
            Prepared::Ready => {
                self.delivering = true;
                self.deliver(input)
            }
            Prepared::Nothing => Ok(None),
            Prepared::Dropped(error) => Err(CodecError::Dropped(error)),
        }
    }

    /// One demultiplexer step that never hands out a payload: it validates a message's start
    /// against the profile, drops an invalid message whole (without gathering its payload into
    /// a buffer, so a hostile length cannot make the reader allocate) and reports whether a
    /// valid start now waits to be delivered.
    fn prepare(&mut self, input: &mut &[u8]) -> Result<Prepared, CodecError> {
        match self.mux.next_item(input, 0) {
            Ok(None) => Ok(Prepared::Nothing),
            Ok(Some(Item::Peek(peek))) => match check_peek(&peek) {
                Ok(channel) => {
                    self.channel = channel;
                    Ok(Prepared::Ready)
                }
                Err(error) => {
                    peek.skip();
                    Ok(Prepared::Dropped(error))
                }
            },
            Ok(Some(Item::Message(_))) => unreachable!(
                "no message completes while its start is still waiting for the profile check"
            ),
            Ok(Some(Item::Wire(_))) => unreachable!("the media reader never forwards a message"),
            Err(error) => {
                let error = CodecError::Malformed(error.kind());
                self.fatal = Some(error);
                Err(error)
            }
        }
    }

    /// Resolves the peek [`prepare`](Self::prepare) left pending: the message itself, or
    /// nothing while a multi-chunk message waits for more input.
    fn deliver<'s, 'a: 's>(
        &'s mut self,
        input: &mut &'a [u8],
    ) -> Result<Option<MediaMessage<'s>>, CodecError> {
        match self.mux.next_item(input, 0) {
            Ok(Some(Item::Message(message))) => {
                self.delivering = false;
                Ok(Some(MediaMessage {
                    channel: self.channel,
                    payload: message.payload,
                }))
            }
            Ok(None) => Ok(None),
            Ok(Some(Item::Peek(_))) => {
                unreachable!("a validated peek resolves straight into its message")
            }
            Ok(Some(Item::Wire(_))) => unreachable!("the media reader never forwards a message"),
            Err(error) => {
                let error = CodecError::Malformed(error.kind());
                self.fatal = Some(error);
                Err(error)
            }
        }
    }

    /// Checks that the input ended at a message boundary; call it when the link closes.
    ///
    /// The document's terminator ends the stream cleanly. Without one, the input still ends
    /// cleanly exactly when no message is half-read: the reader then closes the octet stream
    /// on a synthetic terminator, which the demultiplexer accepts only between messages. Every
    /// other end is [`CodecError::Truncated`].
    pub fn finish(&mut self) -> Result<(), CodecError> {
        let error = match self.mux.finish() {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if let Some(fatal) = self.fatal {
            return Err(fatal);
        }
        if !matches!(
            error.kind(),
            Some(ErrorCode::ReadCompleteChunkFailed | ErrorCode::GotIncompleteBvnrStream)
        ) {
            let error = CodecError::Malformed(error.kind());
            self.fatal = Some(error);
            return Err(error);
        }
        let mut probe: &[u8] = b"\0;";
        loop {
            match self.mux.read(&mut probe) {
                Ok(Some(_message)) => {}
                Ok(None) => break,
                Err(_) => {
                    self.fatal = Some(CodecError::Truncated);
                    return Err(CodecError::Truncated);
                }
            }
        }
        if self.mux.is_done() {
            Ok(())
        } else {
            self.fatal = Some(CodecError::Truncated);
            Err(CodecError::Truncated)
        }
    }

    /// Makes the reader ready for a new stream, keeping its buffers so a warm reader does not
    /// allocate for the next stream's messages either.
    pub fn reset(&mut self) {
        self.mux.reset();
        self.channel = 0;
        self.delivering = false;
        self.fatal = None;
    }
}

impl Default for MediaReader {
    fn default() -> Self {
        Self::new()
    }
}

/// What one [`MediaReader::prepare`] found.
enum Prepared {
    /// A message's start passed the profile check and its payload is pending.
    Ready,
    /// No message completed yet; all the input given was taken.
    Nothing,
    /// A message outside the profile, skipped whole.
    Dropped(MediaError),
}

/// Checks a message's start against the media profile and returns its channel. The declared
/// length is checked before the message is gathered, so an oversized one is never buffered.
fn check_peek(peek: &Peek<'_>) -> Result<u16, MediaError> {
    let channel = u16::try_from(peek.channel).map_err(|_| MediaError::OutOfRange("channel"))?;
    let len = usize::try_from(peek.len).map_err(|_| MediaError::TooLarge)?;
    check_message(channel, len)?;
    Ok(channel)
}

#[cfg(test)]
mod tests {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    use bovnar::stream::mux_dump;

    use super::*;
    use crate::media::{
        MAX_AUDIO_MESSAGE, MAX_PLUGIN_MESSAGE, MAX_VIDEO_LAYERS, MAX_VIDEO_MESSAGE,
    };

    /// Counts allocations per thread, so the no-allocation checks are not disturbed by tests
    /// running in parallel. Same pattern as the other no-allocation tests in the workspace.
    struct CountingAlloc;

    thread_local! {
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    // SAFETY: delegates to the system allocator; the counter is a const-initialised
    // thread-local `Cell`, which never allocates.
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
            // SAFETY: same contract as the caller's.
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: same contract as the caller's.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
            // SAFETY: same contract as the caller's.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static GLOBAL: CountingAlloc = CountingAlloc;

    fn allocations() -> usize {
        ALLOCATIONS.with(Cell::get)
    }

    /// A deterministic payload of `len` bytes with a pattern that differs per `seed`.
    fn payload(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|at| (at as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    /// The full document of `messages`, terminator included.
    fn document(messages: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let writer = MediaWriter::new();
        let mut out = Vec::new();
        writer.begin(&mut out);
        for (channel, payload) in messages {
            writer
                .write_message(&mut out, *channel, payload)
                .expect("valid message");
        }
        writer.finish(&mut out);
        out
    }

    /// Reads a whole document in pieces of `piece` bytes and checks it against `expected`.
    fn read_all(doc: &[u8], piece: usize, expected: &[(u16, Vec<u8>)]) {
        let mut reader = MediaReader::new();
        let mut got = Vec::new();
        for part in doc.chunks(piece) {
            let mut input = part;
            while let Some(message) = reader.read(&mut input).expect("valid stream") {
                got.push((message.channel, message.payload.to_vec()));
            }
            assert!(input.is_empty(), "the reader took every byte of the piece");
        }
        reader.finish().expect("the document ends cleanly");
        assert_eq!(got, expected, "feeding {piece} bytes at a time");
    }

    #[test]
    fn the_fixed_document_opens_as_the_profile_says() {
        let writer = MediaWriter::new();
        assert_eq!(writer.opening(), b"#!bovnar 1.2\n.media=\0");
        assert_eq!(writer.terminator(), b"\0;");
        let mut out = Vec::new();
        writer.begin(&mut out);
        assert_eq!(out, writer.opening());
        writer.finish(&mut out);
        assert_eq!(&out[writer.opening().len()..], writer.terminator());
    }

    #[test]
    fn every_message_size_round_trips() {
        // The chunk boundary of channel 1: the first chunk holds 65,536 minus the channel and
        // length varints (1 + 3 bytes).
        let messages = vec![
            (0u16, Vec::new()),
            (1, payload(150, 1)),
            (6, payload(30_000, 2)),
            (1, payload(65_532, 3)),
            (2, payload(65_533, 4)),
            (3, payload(131_072, 5)),
            (5, payload(150, 6)),
            (0, payload(1_000, 7)),
        ];
        let doc = document(&messages);
        for piece in [1, 2, 3, 5, 7, 64, 1_024, 4_096, 65_536, doc.len()] {
            read_all(&doc, piece, &messages);
        }
    }

    #[test]
    fn a_split_at_every_byte_round_trips() {
        let messages = vec![
            (0u16, b"description".to_vec()),
            (1, payload(100, 7)),
            (5, payload(150, 9)),
            (6, payload(40, 11)),
        ];
        let doc = document(&messages);
        for split in 0..=doc.len() {
            let mut reader = MediaReader::new();
            let mut got = Vec::new();
            for part in [&doc[..split], &doc[split..]] {
                let mut input = part;
                while let Some(message) = reader.read(&mut input).expect("valid stream") {
                    got.push((message.channel, message.payload.to_vec()));
                }
                assert!(input.is_empty());
            }
            reader.finish().expect("the document ends cleanly");
            assert_eq!(got, messages, "split at {split}");
        }
    }

    #[test]
    fn a_terminator_ends_the_stream_cleanly() {
        let messages = vec![(1u16, payload(150, 3)), (5, payload(80, 4))];
        let doc = document(&messages);
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        while reader.read(&mut input).expect("valid stream").is_some() {}
        assert!(input.is_empty());
        assert_eq!(reader.finish(), Ok(()));
        // The reader is done: another read takes nothing.
        assert_eq!(reader.read(&mut input), Ok(None));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn eof_is_clean_exactly_at_a_message_boundary() {
        let writer = MediaWriter::new();
        let mut doc = Vec::new();
        writer.begin(&mut doc);
        let opener_len = doc.len();
        // Prefix 0 (an empty link) and the opening alone are boundaries too.
        let mut boundaries = vec![0, opener_len];
        for (channel, bytes) in [(0u16, &b"control"[..]), (1, b"frame"), (5, b"audio")] {
            writer
                .write_message(&mut doc, channel, bytes)
                .expect("valid message");
            boundaries.push(doc.len());
        }
        // No terminator: the link closed after the last message.
        for end in 0..=doc.len() {
            if end > 0 && end < opener_len {
                // Inside the document opening; not a message boundary.
                continue;
            }
            let mut reader = MediaReader::new();
            let mut input = &doc[..end];
            let mut count = 0;
            while reader.read(&mut input).expect("valid prefix").is_some() {
                count += 1;
            }
            match boundaries.binary_search(&end) {
                Ok(index) => {
                    assert!(reader.finish().is_ok(), "prefix {end} ends at a boundary");
                    // The first boundary is the empty link, before any message.
                    assert_eq!(count, index.saturating_sub(1), "prefix {end}");
                }
                Err(_) => assert_eq!(
                    reader.finish(),
                    Err(CodecError::Truncated),
                    "prefix {end} ends inside the document"
                ),
            }
        }
    }

    #[test]
    fn eof_inside_a_multi_chunk_message_is_truncated() {
        let writer = MediaWriter::new();
        let mut doc = Vec::new();
        writer.begin(&mut doc);
        let opener = doc.len();
        writer
            .write_message(&mut doc, 1, &payload(70_000, 5))
            .expect("valid message");
        // The first chunk of channel 1 holds 65,532 payload bytes after the 7 header bytes.
        let first_chunk_end = opener + 7 + 65_532;
        assert_eq!(&doc[first_chunk_end..first_chunk_end + 1], b"\x01");
        for cut in [
            first_chunk_end,
            first_chunk_end + 1,
            first_chunk_end + 3,
            first_chunk_end + 4,
            doc.len() - 1,
        ] {
            let mut reader = MediaReader::new();
            let mut input = &doc[..cut];
            assert_eq!(reader.read(&mut input).expect("valid framing"), None);
            assert_eq!(reader.finish(), Err(CodecError::Truncated), "cut at {cut}");
        }
        // The message ended exactly where the input did: a clean stop.
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        assert!(reader.read(&mut input).expect("valid stream").is_some());
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn a_malformed_channel_varint_is_malformed() {
        let mut doc = Vec::new();
        MediaWriter::new().begin(&mut doc);
        doc.extend_from_slice(&[0x01, 11, 0]);
        doc.extend_from_slice(&[0x80; 11]);
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Malformed(Some(ErrorCode::OctetStreamOutOfSync)))
        );
    }

    #[test]
    fn a_malformed_length_varint_is_malformed() {
        let mut doc = Vec::new();
        MediaWriter::new().begin(&mut doc);
        doc.extend_from_slice(&[0x01, 12, 0, 0x01]);
        doc.extend_from_slice(&[0x80; 11]);
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Malformed(Some(ErrorCode::OctetStreamOutOfSync)))
        );
    }

    #[test]
    fn an_unknown_chunk_tag_is_malformed() {
        let mut doc = Vec::new();
        MediaWriter::new().begin(&mut doc);
        doc.push(0x02);
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Malformed(Some(ErrorCode::OctetStreamOutOfSync)))
        );
    }

    #[test]
    fn a_declared_length_over_the_reader_cap_is_malformed() {
        let mut doc = Vec::new();
        MediaWriter::new().begin(&mut doc);
        let mut body = vec![0x01];
        varint(MAX_VIDEO_MESSAGE as u64 + 1, &mut body);
        doc.push(0x01);
        doc.extend_from_slice(&(body.len() as u16).to_le_bytes());
        doc.extend_from_slice(&body);
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Malformed(Some(ErrorCode::ValueOutOfRange)))
        );
    }

    #[test]
    fn a_message_outside_the_profile_is_dropped_and_the_stream_goes_on() {
        let oversized = vec![0u8; MAX_AUDIO_MESSAGE + 1];
        let doc = mux_dump(
            &[
                (7u64, &b"reserved"[..]),
                (4_096, &b"out of range"[..]),
                (5, &oversized[..]),
                (1, &b"good"[..]),
            ],
            MEDIA_KEY,
        )
        .expect("valid mux");
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Dropped(MediaError::Invalid("channel")))
        );
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Dropped(MediaError::OutOfRange("channel")))
        );
        assert_eq!(
            reader.read(&mut input),
            Err(CodecError::Dropped(MediaError::TooLarge))
        );
        let message = reader
            .read(&mut input)
            .expect("valid stream")
            .expect("the good message follows");
        assert_eq!(message.channel, 1);
        assert_eq!(message.payload, b"good");
        assert_eq!(reader.read(&mut input), Ok(None));
        assert!(input.is_empty());
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn the_writer_refuses_channels_and_sizes_outside_the_profile() {
        let writer = MediaWriter::new();
        let mut out = Vec::new();
        writer.begin(&mut out);
        let empty = out.len();
        for (channel, error) in [
            (7u16, MediaError::Invalid("channel")),
            (4_096, MediaError::OutOfRange("channel")),
            (u16::MAX, MediaError::OutOfRange("channel")),
        ] {
            assert_eq!(writer.write_message(&mut out, channel, b"x"), Err(error));
            assert_eq!(out.len(), empty, "nothing is written for channel {channel}");
        }
        let oversized_audio = vec![0u8; MAX_AUDIO_MESSAGE + 1];
        let oversized_plugin = vec![0u8; MAX_PLUGIN_MESSAGE + 1];
        assert_eq!(
            writer.write_message(&mut out, 5, &oversized_audio),
            Err(MediaError::TooLarge)
        );
        assert_eq!(
            writer.write_message(&mut out, 6, &oversized_plugin),
            Err(MediaError::TooLarge)
        );
        assert_eq!(out.len(), empty);
        // The caps themselves are inclusive.
        writer
            .write_message(&mut out, 5, &vec![0u8; MAX_AUDIO_MESSAGE])
            .expect("the audio cap is inclusive");
        writer
            .write_message(&mut out, 6, &vec![0u8; MAX_PLUGIN_MESSAGE])
            .expect("the plugin cap is inclusive");
        // Control messages have no cap of their own beyond the reader's message cap.
        writer
            .write_message(&mut out, 0, &vec![0u8; 70_000])
            .expect("control accepts any message within the mux cap");
    }

    #[test]
    fn the_writer_reuses_for_a_new_stream() {
        let writer = MediaWriter::new();
        let mut first = Vec::new();
        writer.begin(&mut first);
        writer
            .write_message(&mut first, 1, b"first")
            .expect("valid message");
        writer.finish(&mut first);
        let mut second = Vec::new();
        writer.begin(&mut second);
        writer
            .write_message(&mut second, 5, b"second")
            .expect("valid message");
        writer.finish(&mut second);
        read_all(&first, 3, &[(1, b"first".to_vec())]);
        read_all(&second, 3, &[(5, b"second".to_vec())]);
        assert_ne!(first, second);
    }

    #[test]
    fn the_reader_resets_for_a_new_stream() {
        let messages = vec![(1u16, payload(150, 1)), (6, payload(30_000, 2))];
        let doc = document(&messages);
        let mut reader = MediaReader::new();
        for _ in 0..2 {
            let mut input = &doc[..];
            let mut got = Vec::new();
            while let Some(message) = reader.read(&mut input).expect("valid stream") {
                got.push((message.channel, message.payload.to_vec()));
            }
            reader.finish().expect("clean stop");
            assert_eq!(got, messages);
            reader.reset();
        }
    }

    /// The payloads of the allocation tests, built before any measuring starts.
    struct AllocationPayloads {
        small: Vec<u8>,
        video: Vec<u8>,
        plugin: Vec<u8>,
    }

    /// Writes the document of the allocation test: a warm-up part and a much longer measured
    /// part. The warm-up grows the writer's output and the reader's per-channel buffers.
    fn write_allocation_document(
        writer: &MediaWriter,
        out: &mut Vec<u8>,
        payloads: &AllocationPayloads,
    ) -> usize {
        let AllocationPayloads {
            small,
            video,
            plugin,
        } = payloads;
        writer.begin(out);
        for channel in [0u16, 1, 5, 6] {
            writer
                .write_message(out, channel, small)
                .expect("valid message");
        }
        writer.write_message(out, 3, video).expect("valid message");
        writer.write_message(out, 6, plugin).expect("valid message");
        let warm_messages = 6;
        for _ in 0..512 {
            for channel in [0u16, 1, 5, 6] {
                writer
                    .write_message(out, channel, small)
                    .expect("valid message");
            }
        }
        for _ in 0..4 {
            writer.write_message(out, 3, video).expect("valid message");
            writer.write_message(out, 6, plugin).expect("valid message");
        }
        writer.finish(out);
        warm_messages
    }

    fn allocation_payloads() -> AllocationPayloads {
        AllocationPayloads {
            small: payload(150, 1),
            video: payload(70_000, 2),
            plugin: payload(MAX_PLUGIN_MESSAGE, 3),
        }
    }

    #[test]
    fn the_reader_allocates_nothing_per_message_once_warm() {
        let writer = MediaWriter::new();
        let payloads = allocation_payloads();
        let mut doc = Vec::new();
        let warm_messages = write_allocation_document(&writer, &mut doc, &payloads);
        let mut reader = MediaReader::new();
        let mut input = &doc[..];
        let mut warmed = 0;
        while warmed < warm_messages {
            reader
                .read(&mut input)
                .expect("valid stream")
                .expect("a warm-up message");
            warmed += 1;
        }
        let start = allocations();
        let mut count = 0;
        let mut bytes = 0;
        while let Some(message) = reader.read(&mut input).expect("valid stream") {
            count += 1;
            bytes += message.payload.len();
        }
        reader.finish().expect("clean stop");
        let spent = allocations() - start;
        assert_eq!(count, 512 * 4 + 4 * 2);
        assert_eq!(bytes, 512 * 4 * 150 + 4 * (70_000 + MAX_PLUGIN_MESSAGE));
        assert_eq!(spent, 0, "the reader allocated {spent} times per message");
    }

    #[test]
    fn the_writer_allocates_nothing_per_message_once_warm() {
        // The first pass grows the output buffer to the document's size; the second reuses it.
        let writer = MediaWriter::new();
        let payloads = allocation_payloads();
        let mut out = Vec::new();
        write_allocation_document(&writer, &mut out, &payloads);
        out.clear();
        let start = allocations();
        write_allocation_document(&writer, &mut out, &payloads);
        let spent = allocations() - start;
        assert_eq!(spent, 0, "the writer allocated {spent} times per message");
    }

    /// Appends `value` as an unsigned LEB128 varint.
    fn varint(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let byte = value as u8 & 0x7F;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    #[test]
    fn every_video_channel_is_accepted_by_the_reader() {
        let messages: Vec<(u16, Vec<u8>)> = (0..MAX_VIDEO_LAYERS)
            .map(|layer| (u16::from(layer) + 1, payload(32, layer)))
            .collect();
        let doc = document(&messages);
        read_all(&doc, 5, &messages);
    }
}
