//! Reads the temporal layer of an encoded AV1 frame from its OBU headers (AV1 spec 5.3).
//!
//! Some hardware encoders (AMF, oneVPL) tell whether a frame is a keyframe but not which temporal
//! layer it belongs to. With temporal layers on, every OBU of a layered frame carries an extension
//! header with that layer's `temporal_id`, so the bitstream itself is the reliable source.

/// Returns the `temporal_id` of the first OBU in `data` that has an extension header, or `None`
/// when no OBU has one (a stream without temporal layers). Fails on a malformed OBU sequence.
pub(super) fn temporal_id(data: &[u8]) -> Result<Option<u8>, &'static str> {
    let mut rest = data;
    while let Some((&header, after_header)) = rest.split_first() {
        if header & 0x80 != 0 {
            return Err("OBU forbidden bit set");
        }
        let has_extension = header & 0x04 != 0;
        let has_size = header & 0x02 != 0;
        if has_extension {
            let extension = after_header.first().ok_or("OBU extension header missing")?;
            return Ok(Some(extension >> 5));
        }
        if !has_size {
            // Without a size field the OBU runs to the end of the data.
            return Ok(None);
        }
        let (size, after_size) = leb128(after_header)?;
        let size = usize::try_from(size).map_err(|_| "OBU size too large")?;
        rest = after_size.get(size..).ok_or("OBU longer than the data")?;
    }
    Ok(None)
}

/// Decodes an unsigned LEB128 number of at most 8 bytes (AV1 spec 4.10.5) and returns it with the
/// bytes after it.
fn leb128(data: &[u8]) -> Result<(u64, &[u8]), &'static str> {
    let mut value = 0u64;
    for (i, byte) in data.iter().take(8).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((value, &data[i + 1..]));
        }
    }
    Err("OBU size field truncated")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPORAL_DELIMITER: u8 = 2 << 3;
    const SEQUENCE_HEADER: u8 = 1 << 3;
    const FRAME: u8 = 6 << 3;
    const HAS_SIZE: u8 = 0x02;
    const HAS_EXTENSION: u8 = 0x04;

    /// An OBU of `kind` with a size field, an optional extension for `temporal_id` and `payload`
    /// bytes.
    fn obu(kind: u8, temporal_id: Option<u8>, payload: usize) -> Vec<u8> {
        let mut out = vec![
            kind | HAS_SIZE
                | if temporal_id.is_some() {
                    HAS_EXTENSION
                } else {
                    0
                },
        ];
        if let Some(id) = temporal_id {
            out.push(id << 5 | 0b0000_1000);
        }
        let mut size = payload;
        loop {
            let byte = (size & 0x7f) as u8;
            size >>= 7;
            if size == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out.extend(std::iter::repeat_n(0xaa, payload));
        out
    }

    #[test]
    fn finds_the_layer_after_unlayered_obus() {
        for id in 0..8 {
            let mut data = obu(TEMPORAL_DELIMITER, None, 0);
            data.extend(obu(SEQUENCE_HEADER, None, 12));
            data.extend(obu(FRAME, Some(id), 300));
            assert_eq!(temporal_id(&data), Ok(Some(id)));
        }
    }

    #[test]
    fn a_stream_without_extensions_has_no_layer() {
        let mut data = obu(TEMPORAL_DELIMITER, None, 0);
        data.extend(obu(FRAME, None, 200));
        assert_eq!(temporal_id(&data), Ok(None));
        assert_eq!(temporal_id(&[]), Ok(None));
    }

    #[test]
    fn an_obu_without_size_runs_to_the_end() {
        let data = [FRAME, 0x11, 0x22, 0x33];
        assert_eq!(temporal_id(&data), Ok(None));
        let data = [FRAME | HAS_EXTENSION, 2 << 5, 0x11];
        assert_eq!(temporal_id(&data), Ok(Some(2)));
    }

    #[test]
    fn rejects_malformed_data() {
        assert!(temporal_id(&[0x80 | FRAME | HAS_SIZE, 0]).is_err());
        assert!(temporal_id(&[FRAME | HAS_EXTENSION]).is_err());
        assert!(temporal_id(&[FRAME | HAS_SIZE, 5, 1, 2]).is_err());
        assert!(temporal_id(&[FRAME | HAS_SIZE, 0x80, 0x80]).is_err());
        assert!(
            temporal_id(&[
                FRAME | HAS_SIZE,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                0xff,
                1
            ])
            .is_err()
        );
    }

    #[test]
    fn reads_multi_byte_sizes() {
        let mut data = obu(SEQUENCE_HEADER, None, 1000);
        data.extend(obu(FRAME, Some(3), 70_000));
        assert_eq!(temporal_id(&data), Ok(Some(3)));
        assert_eq!(leb128(&[0xe5, 0x8e, 0x26, 9]), Ok((624_485, &[9u8][..])));
    }
}
