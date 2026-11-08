//! Prototype of Bovnar's octet-stream multiplexing as a media stream format (#680, step 1 of
//! #492). It exists to measure the framing against today's fixed broadcast header; the client
//! does not use it, and the production version belongs in the Bovnar crate (#681).
//!
//! # Wire format
//!
//! Inside a Bovnar octet stream (opened by `0x00` after an assignment such as `.media = `), the
//! bytes are a sequence of chunks closed by a terminator:
//!
//! | bytes | field                                                                 |
//! |-------|-----------------------------------------------------------------------|
//! | 1     | `0x01` chunk tag (`0x00` is the terminator and ends the stream)       |
//! | 2     | chunk length, little endian; `0` means 65536 (empty chunks are never written) |
//! | n     | chunk payload                                                         |
//!
//! The mux convention (C reference `bvnr_mux_send` / `bvnr_demux_on_event`) gives each chunk
//! payload a LEB128 varint channel first. A message is `varint(len) ++ data` on its channel,
//! so the first chunk of a message is `varint(channel) varint(len) data…` and every following
//! chunk of the same message is `varint(channel) data…`. A chunk holds at most 65536 payload
//! bytes, so messages above roughly 64 KiB span several chunks.
//!
//! # Profile rules (stricter than the C demux)
//!
//! - Every message starts a new chunk and no chunk carries bytes past the end of its message,
//!   so a relay can drop a message by skipping whole chunks. The C demux also accepts packed
//!   chunks; this reader rejects them.
//! - The chunks of one message follow each other on the wire without chunks of another channel
//!   in between. The C demux reassembles interleaved channels; this reader rejects them.
//! - The channel and length varints of a message sit whole in its first chunk (as
//!   `bvnr_mux_send` always writes them).
//!
//! Nothing here allocates per message: the writer hands out chunk headers next to borrowed
//! payload slices, the readers work on whatever bytes the caller has, and the full reader only
//! copies a message into its reusable buffer when the message is not contiguous in the input.

#![forbid(unsafe_code)]

/// The largest chunk payload; its length field is written as 0.
pub const MAX_CHUNK_PAYLOAD: usize = 65536;
/// Opens a chunk.
pub const CHUNK_TAG: u8 = 0x01;
/// Ends the octet stream.
pub const TERMINATOR: u8 = 0x00;
const MAX_VARINT: usize = 10;
/// Tag, length, channel varint and message-length varint.
pub const MAX_CHUNK_HEADER: usize = 3 + 2 * MAX_VARINT;

#[inline]
fn encode_varint(mut value: u64, out: &mut [u8; MAX_VARINT]) -> u8 {
    let mut n = 0;
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out[n] = byte;
            return n as u8 + 1;
        }
        out[n] = byte | 0x80;
        n += 1;
    }
}

fn varint_len(value: u64) -> usize {
    (64 - (value | 1).leading_zeros() as usize).div_ceil(7)
}

/// Frames messages of one channel. Cheap to keep per track: it only holds the encoded channel.
#[derive(Debug, Clone, Copy)]
pub struct MuxWriter {
    channel: [u8; MAX_VARINT],
    channel_len: u8,
}

impl MuxWriter {
    pub fn new(channel: u64) -> Self {
        let mut buf = [0; MAX_VARINT];
        let channel_len = encode_varint(channel, &mut buf);
        Self {
            channel: buf,
            channel_len,
        }
    }

    /// The chunks of one message, each a header and a borrowed slice of `message`, ready for a
    /// vectored write. An empty message is one chunk with an empty body.
    #[inline]
    pub fn chunks<'a>(&self, message: &'a [u8]) -> Chunks<'a> {
        let mut length = [0; MAX_VARINT];
        let length_len = encode_varint(message.len() as u64, &mut length);
        Chunks {
            writer: *self,
            length,
            length_len,
            rest: message,
            first: true,
        }
    }

    /// Framing bytes the chunks of a `message_len` byte message add.
    pub fn framing_len(&self, message_len: usize) -> usize {
        let channel = usize::from(self.channel_len);
        let first_header = 3 + channel + varint_len(message_len as u64);
        let first_room = MAX_CHUNK_PAYLOAD - (first_header - 3);
        let more_chunks = message_len
            .saturating_sub(first_room)
            .div_ceil(MAX_CHUNK_PAYLOAD - channel);
        first_header + more_chunks * (3 + channel)
    }

    /// Writes the whole framed message into `out` and returns the bytes written.
    pub fn encode_into(&self, message: &[u8], out: &mut [u8]) -> Result<usize, BufferTooSmall> {
        let needed = message.len() + self.framing_len(message.len());
        let Some(out) = out.get_mut(..needed) else {
            return Err(BufferTooSmall { needed });
        };
        let mut pos = 0;
        for chunk in self.chunks(message) {
            for part in [chunk.header(), chunk.body()] {
                out[pos..pos + part.len()].copy_from_slice(part);
                pos += part.len();
            }
        }
        Ok(pos)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("output buffer too small, {needed} bytes needed")]
pub struct BufferTooSmall {
    pub needed: usize,
}

/// Iterator over the chunks of one message, see [`MuxWriter::chunks`].
#[derive(Debug, Clone)]
pub struct Chunks<'a> {
    writer: MuxWriter,
    length: [u8; MAX_VARINT],
    length_len: u8,
    rest: &'a [u8],
    first: bool,
}

impl<'a> Chunks<'a> {
    /// Writes the next chunk header into `out` and returns its length and the chunk body (a
    /// slice of the message), or `None` after the last chunk.
    #[inline]
    pub fn write_next(&mut self, out: &mut [u8; MAX_CHUNK_HEADER]) -> Option<(usize, &'a [u8])> {
        if !self.first && self.rest.is_empty() {
            return None;
        }
        // Whole varint buffers are copied (fixed size, so no memcpy call); the bytes past the
        // varint are overwritten or lie outside the header.
        out[3..3 + MAX_VARINT].copy_from_slice(&self.writer.channel);
        let mut header_len = 3 + usize::from(self.writer.channel_len);
        if self.first {
            out[header_len..header_len + MAX_VARINT].copy_from_slice(&self.length);
            header_len += usize::from(self.length_len);
            self.first = false;
        }
        let room = MAX_CHUNK_PAYLOAD - (header_len - 3);
        let (body, rest) = self.rest.split_at(self.rest.len().min(room));
        self.rest = rest;
        // 65536 does not fit 16 bits and is written as 0.
        let payload_len = ((header_len - 3 + body.len()) & 0xFFFF) as u16;
        out[0] = CHUNK_TAG;
        out[1..3].copy_from_slice(&payload_len.to_le_bytes());
        Some((header_len, body))
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Chunk<'a>> {
        let mut header = [0; MAX_CHUNK_HEADER];
        let (header_len, body) = self.write_next(&mut header)?;
        Some(Chunk {
            header,
            header_len: header_len as u8,
            body,
        })
    }
}

/// One chunk on the wire: [`Self::header`] followed by [`Self::body`].
#[derive(Debug, Clone, Copy)]
pub struct Chunk<'a> {
    header: [u8; MAX_CHUNK_HEADER],
    header_len: u8,
    body: &'a [u8],
}

impl<'a> Chunk<'a> {
    pub fn header(&self) -> &[u8] {
        &self.header[..usize::from(self.header_len)]
    }

    pub fn body(&self) -> &'a [u8] {
        self.body
    }
}

/// Malformed or unexpected input. The reader that returned it stays failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("unknown chunk tag {0:#04x}")]
    BadTag(u8),
    #[error("varint does not fit 64 bits")]
    VarintOverflow,
    #[error("chunk ends inside its channel or length varint")]
    ShortChunk,
    #[error("message of {0} bytes exceeds the limit")]
    TooLarge(u64),
    #[error("chunk on channel {got} inside a message on channel {open}")]
    Interleaved { open: u64, got: u64 },
    #[error("chunk carries bytes after the end of its message")]
    PackedChunk,
    #[error("stream ends inside a message")]
    Truncated,
    #[error("bytes after the end of the stream")]
    AfterEnd,
}

#[derive(Debug, Clone, Copy, Default)]
struct Varint {
    value: u64,
    shift: u32,
}

impl Varint {
    #[inline]
    fn push(&mut self, byte: u8) -> Result<Option<u64>, ReadError> {
        // The tenth byte may only carry the top bit of a u64 (as the C decoder checks).
        if self.shift == 63 && byte & 0xFE != 0 {
            return Err(ReadError::VarintOverflow);
        }
        self.value |= u64::from(byte & 0x7F) << self.shift;
        if byte & 0x80 == 0 {
            let value = self.value;
            *self = Self::default();
            return Ok(Some(value));
        }
        self.shift += 7;
        Ok(None)
    }
}

/// A whole varint at the start of `buf`: value and size, or `None` when `buf` ends first.
#[inline]
fn decode_varint(buf: &[u8]) -> Result<Option<(u64, usize)>, ReadError> {
    if let Some(&byte) = buf.first()
        && byte < 0x80
    {
        return Ok(Some((u64::from(byte), 1)));
    }
    let mut varint = Varint::default();
    for (i, &byte) in buf.iter().enumerate() {
        if let Some(value) = varint.push(byte)? {
            return Ok(Some((value, i + 1)));
        }
    }
    Ok(None)
}

/// The chunk at `pos` when all of it is in `input`: its channel, where its payload ends and
/// where the bytes after the channel varint start.
#[inline]
fn chunk_at(input: &[u8], pos: usize) -> Option<(u64, usize, usize)> {
    let [CHUNK_TAG, lo, hi, ..] = *input.get(pos..)? else {
        return None;
    };
    let len = match u16::from_le_bytes([lo, hi]) {
        0 => MAX_CHUNK_PAYLOAD,
        len => usize::from(len),
    };
    let start = pos + 3;
    let end = start + len;
    let (channel, channel_len) = decode_varint(input.get(start..end)?).ok()??;
    Some((channel, end, start + channel_len))
}

/// A message that lies complete at the start of the input.
#[derive(Debug, Clone, Copy)]
struct Whole {
    channel: u64,
    len: u64,
    data_start: usize,
    single_chunk: bool,
    end: usize,
}

/// Reads a whole valid message straight from the slice, the common case on a contiguous
/// buffer. `None` when it is not all there or anything is off; the incremental path then takes
/// over and reports the error, so both paths accept and reject the same streams.
#[inline]
fn whole_message(input: &[u8], max_message: u64) -> Option<Whole> {
    let (channel, mut chunk_end, pos) = chunk_at(input, 0)?;
    let (len, len_len) = decode_varint(&input[pos..chunk_end]).ok()??;
    if len > max_message {
        return None;
    }
    let data_start = pos + len_len;
    let mut left = len.checked_sub((chunk_end - data_start) as u64)?;
    let single_chunk = left == 0;
    while left > 0 {
        let (next_channel, end, data) = chunk_at(input, chunk_end)?;
        if next_channel != channel {
            return None;
        }
        left = left.checked_sub((end - data) as u64)?;
        chunk_end = end;
    }
    Some(Whole {
        channel,
        len,
        data_start,
        single_chunk,
        end: chunk_end,
    })
}

#[derive(Debug, Clone, Copy)]
enum State {
    Tag,
    LenLo,
    LenHi(u8),
    Channel,
    Length,
    Data,
    Ended,
    Failed(ReadError),
}

#[derive(Debug, Clone, Copy)]
enum Token {
    /// All input consumed; feed more.
    Need,
    /// A message starts; a zero-length one is also complete.
    Head {
        channel: u64,
        len: u64,
    },
    /// The last `len` consumed bytes are message data; `last` when the message is complete.
    Data {
        len: usize,
        last: bool,
    },
    End,
}

/// The chunk and message state machine both readers share.
#[derive(Debug, Clone)]
struct Framer {
    state: State,
    chunk_left: u32,
    varint: Varint,
    channel: u64,
    open: bool,
    msg_left: u64,
    max_message: u64,
}

impl Framer {
    fn new(max_message: u64) -> Self {
        Self {
            state: State::Tag,
            chunk_left: 0,
            varint: Varint::default(),
            channel: 0,
            open: false,
            msg_left: 0,
            max_message,
        }
    }

    /// Between messages, so [`whole_message`] may read the next one directly.
    #[inline]
    fn idle(&self) -> bool {
        matches!(self.state, State::Tag) && !self.open
    }

    #[inline]
    fn step(&mut self, input: &[u8]) -> Result<(usize, Token), ReadError> {
        let result = self.step_inner(input);
        if let Err(err) = result {
            self.state = State::Failed(err);
        }
        result
    }

    /// Skips the data of the open message, including the headers of its continuation chunks,
    /// as far as `input` goes, without producing tokens. Leaves anything it cannot read in one
    /// go (a split chunk header) to [`Self::step`].
    #[inline]
    fn skip_open(&mut self, input: &[u8]) -> Result<usize, ReadError> {
        let result = self.skip_open_inner(input);
        if let Err(err) = result {
            self.state = State::Failed(err);
        }
        result
    }

    #[inline]
    fn skip_open_inner(&mut self, input: &[u8]) -> Result<usize, ReadError> {
        let mut pos = 0;
        while self.open {
            match self.state {
                State::Data if self.chunk_left == 0 => self.state = State::Tag,
                State::Data => {
                    let n = (input.len() - pos)
                        .min(self.chunk_left as usize)
                        .min(usize::try_from(self.msg_left).unwrap_or(usize::MAX));
                    if n == 0 {
                        break;
                    }
                    pos += n;
                    self.chunk_left -= n as u32;
                    self.msg_left -= n as u64;
                    if self.msg_left == 0 {
                        self.finish_message()?;
                    }
                }
                State::Tag => match self.chunk_start(&input[pos..])? {
                    Some((n, _)) => pos += n,
                    None => break,
                },
                _ => break,
            }
        }
        Ok(pos)
    }

    /// A chunk ends exactly where its message ends (profile rule).
    #[inline]
    fn finish_message(&mut self) -> Result<(), ReadError> {
        if self.chunk_left != 0 {
            return Err(ReadError::PackedChunk);
        }
        self.open = false;
        self.state = State::Tag;
        Ok(())
    }

    #[inline]
    fn start_message(&mut self, len: u64) -> Result<Token, ReadError> {
        if len > self.max_message {
            return Err(ReadError::TooLarge(len));
        }
        self.msg_left = len;
        self.open = true;
        self.state = State::Data;
        if len == 0 {
            self.finish_message()?;
        }
        Ok(Token::Head {
            channel: self.channel,
            len,
        })
    }

    /// Reads a chunk header and its varints straight from the slice when they are all there.
    /// `None` leaves the state untouched for the byte-by-byte path (which also reports the
    /// errors of a short chunk or a terminator).
    #[inline]
    fn chunk_start(&mut self, input: &[u8]) -> Result<Option<(usize, Option<Token>)>, ReadError> {
        let [CHUNK_TAG, lo, hi, ref rest @ ..] = *input else {
            return Ok(None);
        };
        let chunk_len = match u16::from_le_bytes([lo, hi]) {
            0 => MAX_CHUNK_PAYLOAD,
            len => usize::from(len),
        };
        let body = &rest[..rest.len().min(chunk_len)];
        let Some((channel, channel_len)) = decode_varint(body)? else {
            return Ok(None);
        };
        if self.open {
            if channel != self.channel {
                return Err(ReadError::Interleaved {
                    open: self.channel,
                    got: channel,
                });
            }
            self.chunk_left = (chunk_len - channel_len) as u32;
            self.state = State::Data;
            return Ok(Some((3 + channel_len, None)));
        }
        let Some((len, len_len)) = decode_varint(&body[channel_len..])? else {
            return Ok(None);
        };
        self.channel = channel;
        self.chunk_left = (chunk_len - channel_len - len_len) as u32;
        let head = self.start_message(len)?;
        Ok(Some((3 + channel_len + len_len, Some(head))))
    }

    #[inline]
    fn step_inner(&mut self, input: &[u8]) -> Result<(usize, Token), ReadError> {
        let mut pos = 0;
        loop {
            if let State::Tag = self.state
                && let Some((n, head)) = self.chunk_start(&input[pos..])?
            {
                pos += n;
                match head {
                    Some(head) => return Ok((pos, head)),
                    None => continue,
                }
            }
            if let State::Data = self.state {
                if self.chunk_left == 0 {
                    self.state = State::Tag;
                    continue;
                }
                let available = input.len() - pos;
                if available == 0 {
                    return Ok((pos, Token::Need));
                }
                let n = available
                    .min(self.chunk_left as usize)
                    .min(usize::try_from(self.msg_left).unwrap_or(usize::MAX));
                pos += n;
                self.chunk_left -= n as u32;
                self.msg_left -= n as u64;
                let last = self.msg_left == 0;
                if last {
                    self.finish_message()?;
                }
                return Ok((pos, Token::Data { len: n, last }));
            }
            let state = self.state;
            if let State::Failed(err) = state {
                return Err(err);
            }
            let Some(&byte) = input.get(pos) else {
                return Ok((pos, Token::Need));
            };
            pos += 1;
            match state {
                State::Tag => match byte {
                    CHUNK_TAG => self.state = State::LenLo,
                    TERMINATOR if self.open => return Err(ReadError::Truncated),
                    TERMINATOR => {
                        self.state = State::Ended;
                        return Ok((pos, Token::End));
                    }
                    other => return Err(ReadError::BadTag(other)),
                },
                State::LenLo => self.state = State::LenHi(byte),
                State::LenHi(lo) => {
                    let len = u16::from_le_bytes([lo, byte]);
                    self.chunk_left = if len == 0 {
                        MAX_CHUNK_PAYLOAD as u32
                    } else {
                        u32::from(len)
                    };
                    self.state = State::Channel;
                }
                State::Channel | State::Length => {
                    self.chunk_left -= 1;
                    let value = self.varint.push(byte)?;
                    match (state, value) {
                        (_, None) if self.chunk_left == 0 => return Err(ReadError::ShortChunk),
                        (_, None) => {}
                        (State::Channel, Some(channel)) if self.open => {
                            if channel != self.channel {
                                return Err(ReadError::Interleaved {
                                    open: self.channel,
                                    got: channel,
                                });
                            }
                            self.state = State::Data;
                        }
                        (State::Channel, Some(channel)) => {
                            if self.chunk_left == 0 {
                                return Err(ReadError::ShortChunk);
                            }
                            self.channel = channel;
                            self.state = State::Length;
                        }
                        (_, Some(len)) => return Ok((pos, self.start_message(len)?)),
                    }
                }
                State::Ended => return Err(ReadError::AfterEnd),
                State::Data | State::Failed(_) => unreachable!("handled before reading a byte"),
            }
        }
    }
}

/// What [`MuxReader::read`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// A complete message, borrowed from the input when it was contiguous there and from the
    /// reader's buffer otherwise.
    Message { channel: u64, data: &'a [u8] },
    /// The terminator: the stream ended cleanly.
    End,
}

/// Reassembles whole messages from bytes fed in arbitrary pieces.
#[derive(Debug, Clone)]
pub struct MuxReader {
    framer: Framer,
    buf: Vec<u8>,
    channel: u64,
}

impl MuxReader {
    /// `max_message` bounds the declared length of a message (the buffer only grows with bytes
    /// that actually arrive).
    pub fn new(max_message: u64) -> Self {
        Self {
            framer: Framer::new(max_message),
            buf: Vec::new(),
            channel: 0,
        }
    }

    /// Consumes bytes from `input` up to the end of the next message (or all of it) and returns
    /// how many it consumed. Call again with the rest; `None` means it needs more input.
    pub fn read<'a>(
        &'a mut self,
        input: &'a [u8],
    ) -> Result<(usize, Option<Event<'a>>), ReadError> {
        if self.framer.idle()
            && let Some(whole) = whole_message(input, self.framer.max_message)
            && whole.single_chunk
        {
            let data = &input[whole.data_start..whole.end];
            let channel = whole.channel;
            return Ok((whole.end, Some(Event::Message { channel, data })));
        }
        let mut pos = 0;
        loop {
            let (n, token) = self.framer.step(&input[pos..])?;
            pos += n;
            match token {
                Token::Need => return Ok((pos, None)),
                Token::End => return Ok((pos, Some(Event::End))),
                Token::Head { channel, len: 0 } => {
                    return Ok((pos, Some(Event::Message { channel, data: &[] })));
                }
                Token::Head { channel, .. } => {
                    self.channel = channel;
                    self.buf.clear();
                }
                Token::Data { len, last } => {
                    let data = &input[pos - len..pos];
                    let channel = self.channel;
                    if last && self.buf.is_empty() {
                        return Ok((pos, Some(Event::Message { channel, data })));
                    }
                    self.buf.extend_from_slice(data);
                    if last {
                        let data = &self.buf[..];
                        return Ok((pos, Some(Event::Message { channel, data })));
                    }
                }
            }
        }
    }
}

/// What [`RelayReader::read`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayEvent {
    /// A message starts. `flags` is its first byte (`None` for an empty message); the rest of
    /// the message is skipped without being copied.
    Message {
        channel: u64,
        len: u64,
        flags: Option<u8>,
    },
    End,
}

/// The relay's view: channel and flags byte of each message, everything else skipped.
#[derive(Debug, Clone)]
pub struct RelayReader {
    framer: Framer,
    pending: Option<(u64, u64)>,
}

impl RelayReader {
    pub fn new(max_message: u64) -> Self {
        Self {
            framer: Framer::new(max_message),
            pending: None,
        }
    }

    /// Like [`MuxReader::read`], but reports a message as soon as its flags byte is in and
    /// skips the rest on later calls.
    pub fn read(&mut self, input: &[u8]) -> Result<(usize, Option<RelayEvent>), ReadError> {
        if self.pending.is_none()
            && self.framer.idle()
            && let Some(whole) = whole_message(input, self.framer.max_message)
        {
            let event = RelayEvent::Message {
                channel: whole.channel,
                len: whole.len,
                flags: (whole.len > 0).then(|| input[whole.data_start]),
            };
            return Ok((whole.end, Some(event)));
        }
        let mut pos = 0;
        loop {
            if self.pending.is_none() {
                pos += self.framer.skip_open(&input[pos..])?;
            }
            let (n, token) = self.framer.step(&input[pos..])?;
            pos += n;
            match token {
                Token::Need => return Ok((pos, None)),
                Token::End => return Ok((pos, Some(RelayEvent::End))),
                Token::Head { channel, len: 0 } => {
                    let event = RelayEvent::Message {
                        channel,
                        len: 0,
                        flags: None,
                    };
                    return Ok((pos, Some(event)));
                }
                Token::Head { channel, len } => self.pending = Some((channel, len)),
                Token::Data { len, .. } => {
                    if let Some((channel, message_len)) = self.pending.take() {
                        let event = RelayEvent::Message {
                            channel,
                            len: message_len,
                            flags: Some(input[pos - len]),
                        };
                        return Ok((pos, Some(event)));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: u64 = 4 << 20;

    fn encode(messages: &[(u64, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (channel, data) in messages {
            for chunk in MuxWriter::new(*channel).chunks(data) {
                out.extend_from_slice(chunk.header());
                out.extend_from_slice(chunk.body());
            }
        }
        out.push(TERMINATOR);
        out
    }

    type Messages = Vec<(u64, Vec<u8>)>;

    /// Feeds `input` in pieces of at most `piece` bytes: the messages and whether it ended.
    fn decode_in_pieces(input: &[u8], piece: usize) -> Result<(Messages, bool), ReadError> {
        let mut reader = MuxReader::new(MAX);
        let mut messages = Vec::new();
        let mut ended = false;
        for part in input.chunks(piece) {
            let mut part = part;
            while !part.is_empty() {
                let (n, event) = reader.read(part)?;
                part = &part[n..];
                match event {
                    Some(Event::Message { channel, data }) => {
                        messages.push((channel, data.to_vec()))
                    }
                    Some(Event::End) => ended = true,
                    None => {}
                }
            }
        }
        Ok((messages, ended))
    }

    fn decode_relay(input: &[u8], split: usize) -> Result<Vec<RelayEvent>, ReadError> {
        let mut reader = RelayReader::new(MAX);
        let mut events = Vec::new();
        for mut part in [&input[..split], &input[split..]] {
            while !part.is_empty() {
                let (n, event) = reader.read(part)?;
                part = &part[n..];
                events.extend(event);
            }
        }
        Ok(events)
    }

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn sample() -> Vec<(u64, Vec<u8>)> {
        vec![
            (0, b".kind = \"video\";".to_vec()),
            (2, pattern(150, 1)),
            (300, Vec::new()),
            (5, pattern(1, 7)),
            (2, pattern(200, 3)),
        ]
    }

    #[test]
    fn writes_the_documented_bytes() {
        let mut out = [0; 16];
        let n = MuxWriter::new(5)
            .encode_into(&[1, 0xAA], &mut out)
            .expect("fits");
        assert_eq!(&out[..n], &[0x01, 0x04, 0x00, 0x05, 0x02, 0x01, 0xAA]);
        let n = MuxWriter::new(300)
            .encode_into(&[], &mut out)
            .expect("fits");
        assert_eq!(&out[..n], &[0x01, 0x03, 0x00, 0xAC, 0x02, 0x00]);
    }

    #[test]
    fn framing_len_matches_the_chunks() {
        for channel in [0, 127, 128, 1 << 20] {
            let writer = MuxWriter::new(channel);
            for len in [
                0,
                1,
                150,
                30 * 1024,
                65530,
                65531,
                65532,
                65533,
                65534,
                65535,
                65536,
                400 * 1024,
                131070,
                131071,
            ] {
                let data = vec![0; len];
                let framed: usize = writer.chunks(&data).map(|c| c.header().len()).sum();
                assert_eq!(
                    writer.framing_len(len),
                    framed,
                    "channel {channel} len {len}"
                );
            }
        }
    }

    #[test]
    fn encode_into_reports_a_short_buffer() {
        let mut out = [0; 8];
        let err = MuxWriter::new(1).encode_into(&[0; 8], &mut out);
        assert_eq!(err, Err(BufferTooSmall { needed: 13 }));
    }

    #[test]
    fn round_trips_when_split_at_every_byte() {
        let messages = sample();
        let wire = encode(&messages);
        for split in 0..=wire.len() {
            let mut reader = MuxReader::new(MAX);
            let mut got = Vec::new();
            let mut ended = false;
            for mut part in [&wire[..split], &wire[split..]] {
                while !part.is_empty() {
                    let (n, event) = reader.read(part).expect("valid stream");
                    part = &part[n..];
                    match event {
                        Some(Event::Message { channel, data }) => {
                            got.push((channel, data.to_vec()))
                        }
                        Some(Event::End) => ended = true,
                        None => {}
                    }
                }
            }
            assert_eq!(got, messages, "split at {split}");
            assert!(ended);
        }
        assert_eq!(decode_in_pieces(&wire, 1), Ok((messages, true)));
    }

    #[test]
    fn relay_sees_channel_and_flags_at_every_split() {
        let messages = sample();
        let wire = encode(&messages);
        let mut expected: Vec<RelayEvent> = messages
            .iter()
            .map(|(channel, data)| RelayEvent::Message {
                channel: *channel,
                len: data.len() as u64,
                flags: data.first().copied(),
            })
            .collect();
        expected.push(RelayEvent::End);
        for split in 0..=wire.len() {
            assert_eq!(
                decode_relay(&wire, split),
                Ok(expected.clone()),
                "split at {split}"
            );
        }
    }

    #[test]
    fn borrows_a_contiguous_message() {
        let wire = encode(&[(3, pattern(150, 0))]);
        let mut reader = MuxReader::new(MAX);
        let (n, event) = reader.read(&wire).expect("valid");
        let Some(Event::Message { data, .. }) = event else {
            panic!("expected a message, got {event:?}");
        };
        assert!(wire.as_ptr_range().contains(&data.as_ptr()));
        assert_eq!(n, wire.len() - 1);
    }

    #[test]
    fn messages_above_64_kib_span_chunks() {
        let big = pattern(400 * 1024, 9);
        let messages = vec![
            (2, big.clone()),
            (1, pattern(65534, 2)),
            (1, pattern(65535, 4)),
        ];
        let wire = encode(&messages);
        let chunks = MuxWriter::new(2).chunks(&big).count();
        assert_eq!(chunks, 7);
        assert_eq!(MuxWriter::new(2).framing_len(big.len()), 31);
        assert_eq!(
            decode_in_pieces(&wire, wire.len()),
            Ok((messages.clone(), true))
        );
        assert_eq!(decode_in_pieces(&wire, 1000), Ok((messages.clone(), true)));
        let relay = decode_relay(&wire, 70_000).expect("valid");
        assert_eq!(relay.len(), 4);
        assert_eq!(
            relay[0],
            RelayEvent::Message {
                channel: 2,
                len: big.len() as u64,
                flags: Some(big[0])
            }
        );
    }

    #[test]
    fn a_full_chunk_writes_length_zero() {
        // Channel 1 and a 3-byte length leave 65532 bytes in the first chunk.
        let data = pattern(65532, 0);
        let chunks: Vec<_> = MuxWriter::new(1).chunks(&data).collect();
        assert_eq!(chunks.len(), 1);
        assert_eq!(&chunks[0].header()[..3], &[0x01, 0x00, 0x00]);
        assert_eq!(
            decode_in_pieces(&encode(&[(1, data.clone())]), 7),
            Ok((vec![(1, data)], true))
        );
    }

    #[test]
    fn terminator_ends_the_stream() {
        let wire = [TERMINATOR];
        let mut reader = MuxReader::new(MAX);
        assert_eq!(reader.read(&wire), Ok((1, Some(Event::End))));
        assert_eq!(reader.read(&[CHUNK_TAG]), Err(ReadError::AfterEnd));
        let mut relay = RelayReader::new(MAX);
        assert_eq!(
            relay.read(&[TERMINATOR, CHUNK_TAG]),
            Ok((1, Some(RelayEvent::End)))
        );
        assert_eq!(relay.read(&[CHUNK_TAG]), Err(ReadError::AfterEnd));
    }

    fn first_error(input: &[u8]) -> Option<ReadError> {
        decode_in_pieces(input, input.len()).err()
    }

    #[test]
    fn rejects_malformed_input() {
        // Unknown tag.
        assert_eq!(first_error(&[0x02]), Some(ReadError::BadTag(0x02)));
        // Terminator inside a message: 4 bytes declared, 1 present.
        assert_eq!(
            first_error(&[0x01, 0x03, 0x00, 0x01, 0x04, 0xAA, 0x00]),
            Some(ReadError::Truncated)
        );
        // The chunk ends after the channel, before the message length.
        assert_eq!(
            first_error(&[0x01, 0x01, 0x00, 0x01]),
            Some(ReadError::ShortChunk)
        );
        // The chunk ends inside the channel varint.
        assert_eq!(
            first_error(&[0x01, 0x01, 0x00, 0x81]),
            Some(ReadError::ShortChunk)
        );
        // Two messages packed into one chunk.
        assert_eq!(
            first_error(&[0x01, 0x06, 0x00, 0x01, 0x01, 0xAA, 0x01, 0x01, 0xBB]),
            Some(ReadError::PackedChunk)
        );
        // A continuation chunk on another channel.
        assert_eq!(
            first_error(&[
                0x01, 0x03, 0x00, 0x01, 0x02, 0xAA, 0x01, 0x02, 0x00, 0x02, 0xBB
            ]),
            Some(ReadError::Interleaved { open: 1, got: 2 })
        );
        // A tenth varint byte with more than the top bit of a u64.
        let mut overlong = vec![0x01, 0x10, 0x00];
        overlong.extend([0xFF; 9]);
        overlong.push(0x02);
        assert_eq!(first_error(&overlong), Some(ReadError::VarintOverflow));
        // A declared length above the limit.
        let mut reader = MuxReader::new(100);
        assert_eq!(
            reader.read(&[0x01, 0x03, 0x00, 0x01, 0x65, 0x00]),
            Err(ReadError::TooLarge(101))
        );
        // The error sticks.
        assert_eq!(reader.read(&[0x00]), Err(ReadError::TooLarge(101)));
    }
}
