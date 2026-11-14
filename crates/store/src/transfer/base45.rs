//! Base45 (RFC 9285): bytes as text made only of the 45 characters of the QR alphanumeric mode.

const ALPHABET: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

pub(crate) fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(2) * 3);
    let mut push = |value: usize| out.push(char::from(ALPHABET[value]));
    for pair in data.chunks(2) {
        if let [high, low] = *pair {
            let value = usize::from(high) * 256 + usize::from(low);
            push(value % 45);
            push(value / 45 % 45);
            push(value / (45 * 45));
        } else {
            let value = usize::from(pair[0]);
            push(value % 45);
            push(value / 45);
        }
    }
    out
}

/// `None` for anything that is not canonical Base45. The output is at most two thirds of the
/// input length, so the caller bounds the allocation by bounding the text.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 3 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 3 * 2 + 1);
    for group in bytes.chunks(3) {
        let mut value = 0u32;
        for &character in group.iter().rev() {
            value = value * 45 + digit(character)?;
        }
        if group.len() == 3 {
            out.extend_from_slice(&u16::try_from(value).ok()?.to_be_bytes());
        } else {
            out.push(u8::try_from(value).ok()?);
        }
    }
    Some(out)
}

fn digit(character: u8) -> Option<u32> {
    ALPHABET
        .iter()
        .position(|&candidate| candidate == character)
        .and_then(|index| u32::try_from(index).ok())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn rfc_9285_examples() {
        assert_eq!(encode(b"AB"), "BB8");
        assert_eq!(encode(b"Hello!!"), "%69 VD92EX0");
        assert_eq!(encode(b"base-45"), "UJCLQE7W581");
        assert_eq!(decode("QED8WEX0").as_deref(), Some(&b"ietf!"[..]));
        assert_eq!(encode(b""), "");
        assert_eq!(decode("").as_deref(), Some(&[][..]));
    }

    #[test]
    fn rejects_what_is_not_base45() {
        // One character too many, a value above 65535, a tail above 255, foreign characters.
        for text in ["A", "BB8A", ":::", "::", "bb8", "BB8\n", "BÄ"] {
            assert_eq!(decode(text), None, "{text:?}");
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn round_trips(data in proptest::collection::vec(any::<u8>(), 0..600)) {
            let text = encode(&data);
            prop_assert!(text.bytes().all(|b| ALPHABET.contains(&b)));
            prop_assert_eq!(decode(&text), Some(data));
        }

        #[test]
        fn never_panics_on_any_text(text in ".{0,64}") {
            let _ = decode(&text);
        }
    }
}
