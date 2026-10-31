//! The two Qt encodings the official client's settings use: `QDataStream` (a serialized
//! `QVariant`, base64 in the 1.5 JSON file) and the `QSettings` text format (1.4 INI file,
//! Windows registry strings), where non-string values look like `@ByteArray(...)` or
//! `@Variant(...)`.

/// The subset of `QVariant` the importer reads. Unknown types end the decoding with an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Variant {
    Invalid,
    Bool(bool),
    Int(i64),
    UInt(u64),
    String(String),
    ByteArray(Vec<u8>),
    List(Vec<Variant>),
    Uuid([u8; 16]),
    /// A Windows key in the 1.5 format: scan code plus the "extended" (E0) flag.
    WinKeyboard {
        e0: bool,
        code: u16,
    },
    /// A Windows mouse button in the 1.5 format (Mumble's `InputMouse`: 3 = middle,
    /// 4 = side 1, 5 = side 2).
    WinMouse(i32),
    /// Any other registered type (game controllers, HID devices); kept by name only.
    User(String),
}

/// `QMetaType` ids as written by Qt 5 (`QDataStream::Qt_5_0`).
mod type_id {
    pub const BOOL: u32 = 1;
    pub const INT: u32 = 2;
    pub const UINT: u32 = 3;
    pub const LONG_LONG: u32 = 4;
    pub const ULONG_LONG: u32 = 5;
    pub const LIST: u32 = 9;
    pub const STRING: u32 = 10;
    pub const STRING_LIST: u32 = 11;
    pub const BYTE_ARRAY: u32 = 12;
    pub const UUID: u32 = 30;
    pub const USER: u32 = 127;
}

/// Lists and strings longer than this are treated as damaged data.
const MAX_LEN: usize = 1 << 20;

struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.data.len() < n {
            return None;
        }
        let (head, tail) = self.data.split_at(n);
        self.data = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Option<u64> {
        let b = self.take(8)?;
        let mut bytes = [0; 8];
        bytes.copy_from_slice(b);
        Some(u64::from_be_bytes(bytes))
    }

    /// Length-prefixed bytes; `0xFFFFFFFF` is a null value.
    fn bytes(&mut self) -> Option<Vec<u8>> {
        match self.u32()? {
            u32::MAX => Some(Vec::new()),
            len => {
                let len = usize::try_from(len).ok().filter(|l| *l <= MAX_LEN)?;
                self.take(len).map(<[u8]>::to_vec)
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        let raw = self.bytes()?;
        if raw.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16(&units).ok()
    }

    fn count(&mut self) -> Option<usize> {
        usize::try_from(self.u32()?).ok().filter(|n| *n <= MAX_LEN)
    }

    fn variant(&mut self, depth: u8) -> Option<Variant> {
        if depth > 8 {
            return None;
        }
        let id = self.u32()?;
        let _is_null = self.u8()?;
        Some(match id {
            0 => Variant::Invalid,
            type_id::BOOL => Variant::Bool(self.u8()? != 0),
            type_id::INT => Variant::Int(i64::from(self.u32()?.cast_signed())),
            type_id::UINT => Variant::UInt(u64::from(self.u32()?)),
            type_id::LONG_LONG => Variant::Int(self.u64()?.cast_signed()),
            type_id::ULONG_LONG => Variant::UInt(self.u64()?),
            type_id::STRING => Variant::String(self.string()?),
            type_id::BYTE_ARRAY => Variant::ByteArray(self.bytes()?),
            type_id::STRING_LIST => {
                let n = self.count()?;
                let items = (0..n)
                    .map(|_| self.string().map(Variant::String))
                    .collect::<Option<Vec<_>>>()?;
                Variant::List(items)
            }
            type_id::LIST => {
                let n = self.count()?;
                let items = (0..n)
                    .map(|_| self.variant(depth + 1))
                    .collect::<Option<Vec<_>>>()?;
                Variant::List(items)
            }
            type_id::UUID => {
                let mut uuid = [0; 16];
                uuid.copy_from_slice(self.take(16)?);
                Variant::Uuid(uuid)
            }
            type_id::USER => {
                // The type name is written like a C string: length including the final NUL.
                let mut name = self.bytes()?;
                if name.last() == Some(&0) {
                    name.pop();
                }
                let name = String::from_utf8(name).ok()?;
                match name.as_str() {
                    "InputKeyboard" => Variant::WinKeyboard {
                        e0: self.u8()? != 0,
                        code: self.u16()?,
                    },
                    "InputMouse" => Variant::WinMouse(self.u32()?.cast_signed()),
                    // Its fields are not needed, but the decoder could not skip them reliably.
                    _ => Variant::User(name),
                }
            }
            _ => return None,
        })
    }
}

/// Decodes one `QVariant` written by `QDataStream` (Qt 5 format, as Mumble writes it).
pub fn decode_variant(data: &[u8]) -> Option<Variant> {
    Reader { data }.variant(0)
}

/// A value as `QSettings` stores it in text form (INI file, registry string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextValue {
    String(String),
    Bytes(Vec<u8>),
    Variant(Variant),
}

/// Interprets one `QSettings` text value: `@ByteArray(...)`, `@Variant(...)` (both carry raw
/// bytes, one per character) or a plain string (`@@` escapes a leading `@`).
pub fn text_value(s: &str) -> TextValue {
    let latin1 = |inner: &str| -> Option<Vec<u8>> {
        inner
            .chars()
            .map(|c| u8::try_from(u32::from(c)).ok())
            .collect()
    };
    if let Some(inner) = s
        .strip_prefix("@ByteArray(")
        .and_then(|r| r.strip_suffix(')'))
        && let Some(bytes) = latin1(inner)
    {
        return TextValue::Bytes(bytes);
    }
    if let Some(inner) = s
        .strip_prefix("@Variant(")
        .and_then(|r| r.strip_suffix(')'))
        && let Some(variant) = latin1(inner).as_deref().and_then(decode_variant)
    {
        return TextValue::Variant(variant);
    }
    match s.strip_prefix("@@") {
        Some(rest) => TextValue::String(format!("@{rest}")),
        None => TextValue::String(s.to_owned()),
    }
}

/// Undoes the escaping of a `QSettings` INI value and splits a comma-separated list.
/// Escapes: `\\`, `\"`, `\n` and friends, `\xHHHH` (any number of hex digits) and `\0`
/// octal; quoted parts keep commas and spaces.
pub fn ini_unescape(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = raw.trim().chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                items.push(current.trim_end().to_owned());
                current.clear();
            }
            ' ' if !quoted && current.is_empty() => {}
            '\\' => match chars.next() {
                Some('x') => {
                    let mut code = 0u32;
                    while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
                        code = code.saturating_mul(16).saturating_add(d);
                        chars.next();
                    }
                    current.extend(char::from_u32(code));
                }
                Some(d @ '0'..='7') => {
                    let mut code = d.to_digit(8).unwrap_or(0);
                    while let Some(d) = chars.peek().and_then(|c| c.to_digit(8)) {
                        code = code * 8 + d;
                        chars.next();
                    }
                    current.extend(char::from_u32(code));
                }
                Some('a') => current.push('\u{7}'),
                Some('b') => current.push('\u{8}'),
                Some('f') => current.push('\u{c}'),
                Some('n') => current.push('\n'),
                Some('r') => current.push('\r'),
                Some('t') => current.push('\t'),
                Some('v') => current.push('\u{b}'),
                Some(other) => current.push(other),
                None => {}
            },
            c => current.push(c),
        }
    }
    items.push(current);
    items
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Test helper: writes a `QVariant` the way `QDataStream` does.
    pub(crate) fn encode(v: &Variant) -> Vec<u8> {
        fn bytes(out: &mut Vec<u8>, b: &[u8]) {
            out.extend(u32::try_from(b.len()).unwrap_or(0).to_be_bytes());
            out.extend(b);
        }
        let mut out = Vec::new();
        let head = |out: &mut Vec<u8>, id: u32| {
            out.extend(id.to_be_bytes());
            out.push(0);
        };
        match v {
            Variant::Invalid => head(&mut out, 0),
            Variant::Bool(b) => {
                head(&mut out, type_id::BOOL);
                out.push(u8::from(*b));
            }
            Variant::Int(i) => {
                head(&mut out, type_id::INT);
                out.extend(i32::try_from(*i).unwrap_or(0).to_be_bytes());
            }
            Variant::UInt(u) => {
                head(&mut out, type_id::UINT);
                out.extend(u32::try_from(*u).unwrap_or(0).to_be_bytes());
            }
            Variant::String(s) => {
                head(&mut out, type_id::STRING);
                let utf16: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
                bytes(&mut out, &utf16);
            }
            Variant::ByteArray(b) => {
                head(&mut out, type_id::BYTE_ARRAY);
                bytes(&mut out, b);
            }
            Variant::List(items) => {
                head(&mut out, type_id::LIST);
                out.extend(u32::try_from(items.len()).unwrap_or(0).to_be_bytes());
                for item in items {
                    out.extend(encode(item));
                }
            }
            Variant::Uuid(u) => {
                head(&mut out, type_id::UUID);
                out.extend(u);
            }
            Variant::WinKeyboard { e0, code } => {
                head(&mut out, type_id::USER);
                bytes(&mut out, b"InputKeyboard\0");
                out.push(u8::from(*e0));
                out.extend(code.to_be_bytes());
            }
            Variant::WinMouse(m) => {
                head(&mut out, type_id::USER);
                bytes(&mut out, b"InputMouse\0");
                out.extend(m.to_be_bytes());
            }
            Variant::User(name) => {
                head(&mut out, type_id::USER);
                bytes(&mut out, format!("{name}\0").as_bytes());
            }
        }
        out
    }

    /// Test helper: the `QSettings` INI form of a variant, every byte escaped (an escape is
    /// always followed by a backslash or the closing parenthesis, so none run together).
    pub(crate) fn ini_variant(v: &Variant) -> String {
        let escaped: String = encode(v).iter().map(|b| format!("\\x{b:x}")).collect();
        format!("@Variant({escaped})")
    }

    #[test]
    fn variants_round_trip() {
        let values = [
            Variant::UInt(65),
            Variant::Int(-3),
            Variant::Bool(true),
            Variant::String("Zoë".into()),
            Variant::ByteArray(vec![0, 1, 2, 255]),
            Variant::List(vec![Variant::UInt(0x1d00), Variant::Uuid([7; 16])]),
            Variant::WinKeyboard {
                e0: true,
                code: 0x1d,
            },
            Variant::WinMouse(4),
            Variant::User("InputXinput".into()),
        ];
        for value in values {
            assert_eq!(decode_variant(&encode(&value)), Some(value));
        }
    }

    #[test]
    fn damaged_variants_are_refused() {
        let mut data = encode(&Variant::String("abc".into()));
        data.truncate(data.len() - 1);
        assert_eq!(decode_variant(&data), None);
        assert_eq!(decode_variant(&[0, 0, 0, 99, 0]), None);
        assert_eq!(decode_variant(&[]), None);
    }

    #[test]
    fn ini_values_are_unescaped_and_split() {
        assert_eq!(ini_unescape("hello"), vec!["hello"]);
        assert_eq!(ini_unescape("37, 65"), vec!["37", "65"]);
        assert_eq!(ini_unescape("\"a, b\""), vec!["a, b"]);
        assert_eq!(ini_unescape("a\\\\b\\n"), vec!["a\\b\n"]);
        assert_eq!(ini_unescape("\\0\\x1f\\x100"), vec!["\0\u{1f}\u{100}"]);
    }

    #[test]
    fn text_values_carry_bytes_and_variants() {
        assert_eq!(
            text_value("@ByteArray(\0\u{ff}a)"),
            TextValue::Bytes(vec![0, 255, b'a'])
        );
        let v = Variant::List(vec![Variant::UInt(37), Variant::UInt(65)]);
        let raw = ini_unescape(&ini_variant(&v)).remove(0);
        assert_eq!(text_value(&raw), TextValue::Variant(v));
        assert_eq!(text_value("@@at"), TextValue::String("@at".into()));
        assert_eq!(text_value("plain"), TextValue::String("plain".into()));
    }
}
