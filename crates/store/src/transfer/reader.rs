//! The receiving side: the phone collects scanned parts and opens them with the typed code.

use std::fmt;

use super::envelope::{self, HEADER_LEN, Header, ID_LEN, KdfBand, Opened};
use super::parts::Part;
use super::payload::TransferPayload;
use super::{MAX_ATTEMPTS, TransferError};

/// How far the scan is: "Code `have` of `total`" (FR-5.65).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Different parts collected so far.
    pub have: u8,
    /// Parts the transfer consists of.
    pub total: u8,
}

impl Progress {
    /// All parts are there; the code can be asked for.
    pub fn is_complete(&self) -> bool {
        self.have == self.total
    }
}

/// Collects the parts of one transfer and opens it.
///
/// Feed every scanned text to [`add_part`](Self::add_part), in any order; the same code may be
/// scanned many times. When the progress is complete, ask for the code and call
/// [`open`](Self::open). Whenever an error says that the collected parts are discarded
/// ([`Expired`](TransferError::Expired), [`ClockSkew`](TransferError::ClockSkew),
/// [`RescanRequired`](TransferError::RescanRequired), [`Malformed`](TransferError::Malformed)),
/// the reader is empty again and the scan starts over. `Debug` prints the progress only.
pub struct TransferReader {
    band: KdfBand,
    scan: Option<Scan>,
}

/// The parts of the transfer being collected.
struct Scan {
    id: [u8; ID_LEN],
    chunks: Vec<Option<Vec<u8>>>,
    have: u8,
    /// Known once part 0 was scanned.
    header: Option<Header>,
    wrong_codes: u8,
}

impl Scan {
    fn progress(&self) -> Progress {
        Progress {
            have: self.have,
            total: self.chunks.len() as u8,
        }
    }
}

impl Default for TransferReader {
    fn default() -> Self {
        Self::new()
    }
}

impl TransferReader {
    pub fn new() -> Self {
        Self {
            band: KdfBand::PRODUCTION,
            scan: None,
        }
    }

    /// A reader that also accepts the fast key derivation of the tests.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self {
            band: KdfBand::TEST,
            scan: None,
        }
    }

    /// Adds one scanned QR text. `now` is [`unix_now`](super::unix_now).
    ///
    /// A part that was scanned before changes nothing. Errors leave the collected parts as they
    /// are, except [`Expired`](TransferError::Expired), [`ClockSkew`](TransferError::ClockSkew)
    /// and [`Malformed`](TransferError::Malformed), which discard them. On
    /// [`OtherTransfer`](TransferError::OtherTransfer) the desktop most likely created a new
    /// transfer: call [`reset`](Self::reset) and add the part again.
    pub fn add_part(&mut self, text: &str, now: u64) -> Result<Progress, TransferError> {
        let part = Part::decode(text)?;
        if let Some(scan) = &self.scan {
            if scan.id != part.id {
                return Err(TransferError::OtherTransfer);
            }
            if scan.chunks.len() != usize::from(part.total) {
                return Err(TransferError::MalformedPart);
            }
            if scan.chunks[usize::from(part.index)].is_some() {
                return Ok(scan.progress());
            }
        }
        // Part 0 starts with the header: check it before anything is kept, so that an expired
        // or foreign transfer is reported at the first chance and not after the last code.
        let header = if part.index == 0 {
            let header = Header::parse(&part.chunk, &self.band).map_err(|e| match e {
                TransferError::Malformed => TransferError::MalformedPart,
                other => other,
            })?;
            if header.id != part.id || header.total_parts != part.total {
                return Err(TransferError::MalformedPart);
            }
            if let Err(e) = header.check_time(now) {
                self.scan = None;
                return Err(e);
            }
            Some(header)
        } else {
            None
        };

        let scan = self.scan.get_or_insert_with(|| Scan {
            id: part.id,
            chunks: vec![None; usize::from(part.total)],
            have: 0,
            header: None,
            wrong_codes: 0,
        });
        if header.is_some() {
            scan.header = header;
        }
        scan.chunks[usize::from(part.index)] = Some(part.chunk);
        scan.have += 1;
        let progress = scan.progress();
        if progress.is_complete() && self.stream().is_none() {
            // Every part passed its checksum, but together they are not what the header says.
            self.scan = None;
            return Err(TransferError::Malformed);
        }
        Ok(progress)
    }

    /// Where the scan stands; `None` before the first part.
    pub fn progress(&self) -> Option<Progress> {
        self.scan.as_ref().map(Scan::progress)
    }

    /// How many digits the code has; known once the first QR code of the sequence was scanned
    /// (always when the progress is complete).
    pub fn code_digits(&self) -> Option<u8> {
        self.scan.as_ref()?.header.map(|header| header.code_digits)
    }

    /// Wrong codes that are still allowed before the scan must be repeated.
    pub fn attempts_left(&self) -> u8 {
        let used = self.scan.as_ref().map_or(0, |scan| scan.wrong_codes);
        MAX_ATTEMPTS.saturating_sub(used)
    }

    /// Forgets everything collected so far.
    pub fn reset(&mut self) {
        self.scan = None;
    }

    /// Opens the complete transfer with the typed code. `now` is [`unix_now`](super::unix_now).
    /// Takes one to two seconds on a phone (the key derivation), so call it off the UI thread.
    ///
    /// A wrong code gives [`WrongCode`](TransferError::WrongCode) with the attempts that are
    /// left; the third one discards the parts and gives
    /// [`RescanRequired`](TransferError::RescanRequired). A code of the wrong length is
    /// [`InvalidCode`](TransferError::InvalidCode) and not counted. On success the reader is
    /// empty again.
    pub fn open(&mut self, code: &str, now: u64) -> Result<TransferPayload, TransferError> {
        let (header, stream) = self.stream().ok_or(TransferError::Incomplete)?;
        if let Err(e) = header.check_time(now) {
            self.scan = None;
            return Err(e);
        }
        if code.len() != usize::from(header.code_digits)
            || !code.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(TransferError::InvalidCode {
                digits: header.code_digits,
            });
        }
        match envelope::open(&header, code, &stream[HEADER_LEN..]) {
            Ok(Opened::Payload(payload)) => {
                self.scan = None;
                Ok(payload)
            }
            Ok(Opened::WrongCode) => {
                let scan = self.scan.as_mut().ok_or(TransferError::Incomplete)?;
                scan.wrong_codes += 1;
                if scan.wrong_codes >= MAX_ATTEMPTS {
                    self.scan = None;
                    return Err(TransferError::RescanRequired);
                }
                Err(TransferError::WrongCode {
                    attempts_left: MAX_ATTEMPTS - scan.wrong_codes,
                })
            }
            // Not enough memory for the key derivation says nothing about the code or the
            // parts; the user can free memory and try again.
            Err(TransferError::OutOfMemory) => Err(TransferError::OutOfMemory),
            Err(e) => {
                self.scan = None;
                Err(e)
            }
        }
    }

    /// Header and the whole stream when all parts are there and fit together.
    fn stream(&self) -> Option<(Header, Vec<u8>)> {
        let scan = self.scan.as_ref()?;
        let header = scan.header?;
        let mut stream = Vec::new();
        for chunk in &scan.chunks {
            stream.extend_from_slice(chunk.as_ref()?);
        }
        (stream.len() == HEADER_LEN + header.ciphertext_len as usize).then_some((header, stream))
    }
}

impl fmt::Debug for TransferReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransferReader")
            .field("progress", &self.progress())
            .field("attempts_left", &self.attempts_left())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use zeroize::Zeroizing;

    use super::super::envelope::SALT_LEN;
    use super::super::envelope::tests::FAST_KDF;
    use super::super::parts::PART_OVERHEAD;
    use super::super::payload::tests::sample;
    use super::super::writer::tests::{NOW, fast_options, fast_transfer};
    use super::super::writer::{Randomness, Transfer, TransferOptions};
    use super::super::{CLOCK_SKEW_SECS, DEFAULT_CODE_DIGITS, KdfParams, VALIDITY_SECS, base45};
    use super::*;

    fn wrong(code: &str) -> String {
        // Another code of the same length.
        code.bytes()
            .map(|digit| char::from(b'0' + (digit - b'0' + 1) % 10))
            .collect()
    }

    fn scanned(transfer: &Transfer) -> TransferReader {
        let mut reader = TransferReader::for_tests();
        for part in transfer.parts() {
            reader.add_part(part, NOW).expect("add part");
        }
        reader
    }

    #[test]
    fn round_trips_in_order() {
        for payload in [
            sample(1, None),
            sample(3, None),
            sample(1, Some(20)),
            sample(0, Some(3)),
        ] {
            let transfer = fast_transfer(&payload, &fast_options());
            let mut reader = TransferReader::for_tests();
            assert_eq!(reader.progress(), None);
            let total = transfer.parts().len() as u8;
            for (index, part) in transfer.parts().iter().enumerate() {
                let progress = reader.add_part(part, NOW).expect("add part");
                assert_eq!(progress.total, total);
                assert_eq!(usize::from(progress.have), index + 1);
                assert_eq!(progress.is_complete(), index + 1 == usize::from(total));
            }
            assert_eq!(reader.code_digits(), Some(DEFAULT_CODE_DIGITS));
            assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
            // Opened once, nothing is kept.
            assert_eq!(reader.progress(), None);
        }
    }

    #[test]
    fn parts_arrive_in_any_order_and_repeatedly() {
        let payload = sample(3, Some(20));
        let transfer = fast_transfer(&payload, &fast_options());
        let parts = transfer.parts();
        assert!(parts.len() >= 5);
        let mut reader = TransferReader::for_tests();
        // Backwards, every part twice in a row, like a camera that sees each code for a while.
        let mut have = 0;
        for part in parts.iter().rev() {
            have += 1;
            for _ in 0..2 {
                let progress = reader.add_part(part, NOW).expect("add part");
                assert_eq!(progress.have, have);
            }
            // The header is in part 0, which comes last here.
            assert_eq!(
                reader.code_digits().is_some(),
                usize::from(have) == parts.len()
            );
        }
        assert_eq!(
            reader.add_part(&parts[2], NOW).map(|p| p.is_complete()),
            Ok(true)
        );
        assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
    }

    #[test]
    fn open_needs_every_part() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        let mut reader = TransferReader::for_tests();
        assert_eq!(reader.open("123456", NOW), Err(TransferError::Incomplete));
        for part in &transfer.parts()[1..] {
            reader.add_part(part, NOW).expect("add part");
        }
        assert_eq!(
            reader.open(transfer.code(), NOW),
            Err(TransferError::Incomplete)
        );
        reader
            .add_part(&transfer.parts()[0], NOW)
            .expect("add part");
        assert!(reader.open(transfer.code(), NOW).is_ok());
    }

    #[test]
    fn parts_of_another_transfer_are_refused_and_do_no_harm() {
        let payload = sample(1, Some(5));
        let first = fast_transfer(&payload, &fast_options());
        let second = fast_transfer(&payload, &fast_options());
        let mut reader = TransferReader::for_tests();
        reader.add_part(&first.parts()[0], NOW).expect("add part");
        for part in second.parts() {
            assert_eq!(
                reader.add_part(part, NOW),
                Err(TransferError::OtherTransfer)
            );
        }
        for part in &first.parts()[1..] {
            reader.add_part(part, NOW).expect("add part");
        }
        // After a reset the new transfer is read from the start.
        reader.reset();
        for part in second.parts() {
            reader.add_part(part, NOW).expect("add part");
        }
        assert_eq!(reader.open(second.code(), NOW), Ok(payload));
    }

    #[test]
    fn a_part_spliced_in_from_another_transfer_fails_the_decryption() {
        // Same id and part count, different salt and content: the part framing cannot tell them
        // apart, the authenticated header does.
        let build = |salt: u8, seed: usize| {
            let randomness = Randomness {
                id: [5; ID_LEN],
                salt: [salt; SALT_LEN],
                code: Zeroizing::new("111111".to_owned()),
            };
            Transfer::build(
                &sample(seed, None),
                NOW,
                &fast_options(),
                &KdfBand::TEST,
                randomness,
            )
            .expect("build")
        };
        let ours = build(1, 1);
        let theirs = build(2, 1);
        assert_eq!(ours.parts().len(), theirs.parts().len());
        let mut reader = TransferReader::for_tests();
        reader.add_part(&ours.parts()[0], NOW).expect("add part");
        for part in &theirs.parts()[1..] {
            reader.add_part(part, NOW).expect("add part");
        }
        assert_eq!(
            reader.open("111111", NOW),
            Err(TransferError::WrongCode { attempts_left: 2 })
        );
    }

    #[test]
    fn three_wrong_codes_require_a_new_scan() {
        let payload = sample(1, None);
        let transfer = fast_transfer(&payload, &fast_options());
        let bad = wrong(transfer.code());
        let mut reader = scanned(&transfer);
        assert_eq!(reader.attempts_left(), 3);
        assert_eq!(
            reader.open(&bad, NOW),
            Err(TransferError::WrongCode { attempts_left: 2 })
        );
        assert_eq!(
            reader.open(&bad, NOW),
            Err(TransferError::WrongCode { attempts_left: 1 })
        );
        assert_eq!(reader.attempts_left(), 1);
        assert_eq!(reader.open(&bad, NOW), Err(TransferError::RescanRequired));
        // Everything is gone: even the right code needs a new scan.
        assert_eq!(reader.progress(), None);
        assert_eq!(
            reader.open(transfer.code(), NOW),
            Err(TransferError::Incomplete)
        );

        let mut reader = scanned(&transfer);
        assert_eq!(reader.attempts_left(), 3);
        assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
    }

    #[test]
    fn the_right_code_works_on_the_third_attempt() {
        let payload = sample(1, None);
        let transfer = fast_transfer(&payload, &fast_options());
        let bad = wrong(transfer.code());
        let mut reader = scanned(&transfer);
        assert!(reader.open(&bad, NOW).is_err());
        assert!(reader.open(&bad, NOW).is_err());
        assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
    }

    #[test]
    fn a_code_of_the_wrong_shape_is_not_an_attempt() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        let mut reader = scanned(&transfer);
        for code in [
            "",
            "123456",
            "123456789",
            "1234567a",
            "1234 678",
            "１２３４５６７８",
        ] {
            assert_eq!(
                reader.open(code, NOW),
                Err(TransferError::InvalidCode {
                    digits: DEFAULT_CODE_DIGITS
                }),
                "{code:?}"
            );
        }
        assert_eq!(reader.attempts_left(), 3);
        assert!(reader.open(transfer.code(), NOW).is_ok());
    }

    #[test]
    fn longer_codes_are_carried_in_the_header() {
        for digits in [6, 10] {
            let options = TransferOptions {
                code_digits: digits,
                ..fast_options()
            };
            let payload = sample(1, None);
            let transfer = fast_transfer(&payload, &options);
            assert_eq!(transfer.code().len(), usize::from(digits));
            let mut reader = scanned(&transfer);
            assert_eq!(reader.code_digits(), Some(digits));
            assert_eq!(
                reader.open("12345678", NOW),
                Err(TransferError::InvalidCode { digits })
            );
            assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
        }
    }

    #[test]
    fn other_part_sizes_are_read_too() {
        let payload = sample(1, Some(20));
        for part_bytes in [64, 200, 646, 1024] {
            let options = TransferOptions {
                part_bytes,
                ..fast_options()
            };
            let transfer = fast_transfer(&payload, &options);
            let mut reader = scanned(&transfer);
            assert_eq!(
                reader.open(transfer.code(), NOW),
                Ok(payload.clone()),
                "{part_bytes}"
            );
        }
    }

    #[test]
    fn an_expired_transfer_is_refused_when_scanning_and_when_opening() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        let last_valid = NOW + VALIDITY_SECS + CLOCK_SKEW_SECS;

        // Scanning: reported at part 0, and the parts scanned before are dropped.
        let mut reader = TransferReader::for_tests();
        reader
            .add_part(&transfer.parts()[1], last_valid + 1)
            .expect("add part");
        assert_eq!(
            reader.add_part(&transfer.parts()[0], last_valid + 1),
            Err(TransferError::Expired)
        );
        assert_eq!(reader.progress(), None);

        // Opening: scanned in time, code typed too late.
        let mut reader = scanned(&transfer);
        assert_eq!(
            reader.open(transfer.code(), last_valid + 1),
            Err(TransferError::Expired)
        );
        assert_eq!(reader.progress(), None);

        // The last valid second still works.
        let mut reader = TransferReader::for_tests();
        for part in transfer.parts() {
            reader.add_part(part, last_valid).expect("add part");
        }
        assert!(reader.open(transfer.code(), last_valid).is_ok());
    }

    #[test]
    fn clocks_that_differ_too_much_are_reported() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        // The phone's clock is behind the desktop's: within the allowance it works.
        let mut reader = TransferReader::for_tests();
        for part in transfer.parts() {
            reader
                .add_part(part, NOW - CLOCK_SKEW_SECS)
                .expect("add part");
        }
        assert!(reader.open(transfer.code(), NOW - CLOCK_SKEW_SECS).is_ok());

        let mut reader = TransferReader::for_tests();
        assert_eq!(
            reader.add_part(&transfer.parts()[0], NOW - CLOCK_SKEW_SECS - 1),
            Err(TransferError::ClockSkew)
        );
    }

    #[test]
    fn a_production_reader_refuses_a_weak_key_derivation() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        let mut reader = TransferReader::new();
        assert_eq!(
            reader.add_part(&transfer.parts()[0], NOW),
            Err(TransferError::MalformedPart)
        );
        assert_eq!(reader.progress(), None);
    }

    #[test]
    fn damaged_parts_are_reported_and_can_be_scanned_again() {
        let payload = sample(1, None);
        let transfer = fast_transfer(&payload, &fast_options());
        let mut reader = TransferReader::for_tests();
        reader
            .add_part(&transfer.parts()[0], NOW)
            .expect("add part");

        let mut damaged = base45::decode(&transfer.parts()[1]).expect("base45");
        damaged[40] ^= 0x10;
        assert_eq!(
            reader.add_part(&base45::encode(&damaged), NOW),
            Err(TransferError::ChecksumMismatch)
        );
        assert_eq!(
            reader.add_part("WIFI:S:home;;", NOW),
            Err(TransferError::MalformedPart)
        );
        assert_eq!(reader.progress().map(|p| p.have), Some(1));

        for part in &transfer.parts()[1..] {
            reader.add_part(part, NOW).expect("add part");
        }
        assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
    }

    /// Rebuilds a part with a valid checksum after `change`, as a forger would.
    fn forged(part: &str, change: impl FnOnce(&mut Part)) -> String {
        let mut part = Part::decode(part).expect("decode");
        change(&mut part);
        part.encode()
    }

    #[test]
    fn forged_parts_with_valid_checksums_are_caught() {
        let payload = sample(1, None);
        let transfer = fast_transfer(&payload, &fast_options());
        let parts = transfer.parts();

        // A changed ciphertext byte: looks like a wrong code.
        let mut reader = TransferReader::for_tests();
        reader.add_part(&parts[0], NOW).expect("add part");
        reader
            .add_part(&forged(&parts[1], |part| part.chunk[3] ^= 1), NOW)
            .expect("add part");
        for part in &parts[2..] {
            reader.add_part(part, NOW).expect("add part");
        }
        assert_eq!(
            reader.open(transfer.code(), NOW),
            Err(TransferError::WrongCode { attempts_left: 2 })
        );

        // Changed header fields in part 0 that still parse: creation time moved forward to
        // stretch the validity, a longer code announced, a cheaper key derivation.
        let header_changes: [&dyn Fn(&mut Part); 3] = [
            &|part| part.chunk[31] = part.chunk[31].wrapping_add(20),
            &|part| part.chunk[30] = 9,
            &|part| part.chunk[28] = 2,
        ];
        for change in header_changes {
            let mut reader = TransferReader::for_tests();
            reader
                .add_part(&forged(&parts[0], change), NOW)
                .expect("add part");
            for part in &parts[1..] {
                reader.add_part(part, NOW).expect("add part");
            }
            let digits = reader.code_digits().expect("digits");
            let code = format!("{:0<width$}", transfer.code(), width = usize::from(digits));
            assert_eq!(
                reader.open(&code, NOW),
                Err(TransferError::WrongCode { attempts_left: 2 })
            );
        }

        // A part count or id that disagrees with the header, a chunk that is too short.
        let mut reader = TransferReader::for_tests();
        assert_eq!(
            reader.add_part(&forged(&parts[0], |part| part.total += 1), NOW),
            Err(TransferError::MalformedPart)
        );
        assert_eq!(
            reader.add_part(&forged(&parts[0], |part| part.id[0] ^= 1), NOW),
            Err(TransferError::MalformedPart)
        );
        assert_eq!(
            reader.add_part(
                &forged(&parts[0], |part| part.chunk.truncate(HEADER_LEN - 1)),
                NOW
            ),
            Err(TransferError::MalformedPart)
        );
        assert_eq!(reader.progress(), None);

        // A later part announcing another total than the parts before it.
        reader.add_part(&parts[0], NOW).expect("add part");
        assert_eq!(
            reader.add_part(&forged(&parts[1], |part| part.total += 1), NOW),
            Err(TransferError::MalformedPart)
        );

        // All parts there, but one chunk is shorter than the header promises.
        reader
            .add_part(&forged(&parts[1], |part| part.chunk.truncate(10)), NOW)
            .expect("add part");
        let mut result = Ok(Progress { have: 0, total: 0 });
        for part in &parts[2..] {
            result = reader.add_part(part, NOW);
        }
        assert_eq!(result, Err(TransferError::Malformed));
        assert_eq!(reader.progress(), None);
    }

    #[test]
    fn debug_output_shows_progress_only() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        let reader = scanned(&transfer);
        let text = format!("{reader:?}");
        assert!(text.contains("progress"));
        assert!(text.len() < 200, "{text}");
    }

    #[test]
    fn the_default_settings_round_trip_with_the_real_key_derivation() {
        let payload = sample(1, Some(20));
        let transfer = Transfer::create(&payload, NOW).expect("create");
        assert_eq!(transfer.code().len(), usize::from(DEFAULT_CODE_DIGITS));
        let mut reader = TransferReader::new();
        for part in transfer.parts() {
            reader.add_part(part, NOW).expect("add part");
        }
        assert_eq!(
            reader.open(&wrong(transfer.code()), NOW),
            Err(TransferError::WrongCode { attempts_left: 2 })
        );
        assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
    }

    /// The test vector for other implementations and later versions: fixed id, salt, code (six
    /// digits, the shortest a reader accepts) and time with the default key derivation. Only the reading direction is pinned, because the
    /// compressed bytes may differ between versions of the DEFLATE library.
    #[test]
    fn known_answer_vector_v1() {
        let vector = include_str!("vectors/v1.txt");
        let field = |name: &str| {
            vector
                .lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix(": "))
                .unwrap_or_else(|| panic!("the vector has no field {name}"))
        };
        let created_at: u64 = field("created_at").parse().expect("created_at");
        let parts: Vec<&str> = vector
            .lines()
            .filter_map(|line| line.strip_prefix("part: "))
            .collect();
        assert_eq!(parts.len().to_string(), field("part_count"));

        let mut reader = TransferReader::new();
        for part in parts.iter().rev() {
            reader.add_part(part, created_at).expect("add part");
        }
        assert_eq!(
            reader.open(field("wrong_code"), created_at),
            Err(TransferError::WrongCode { attempts_left: 2 })
        );
        assert_eq!(reader.open(field("code"), created_at), Ok(vector_payload()));
    }

    /// The content of `vectors/v1.txt`, described in that file.
    fn vector_payload() -> TransferPayload {
        sample(1, Some(2))
    }

    /// Writes `vectors/v1.txt` anew. Only for a new format version; the checked-in file must
    /// otherwise never change. Run with
    /// `cargo test -p mumcord-store -- --ignored write_known_answer_vector --nocapture`.
    #[test]
    #[ignore = "prints a new test vector"]
    fn write_known_answer_vector() {
        let randomness = Randomness {
            id: [0xA1, 0xB2, 0xC3, 0xD4],
            salt: std::array::from_fn(|i| i as u8),
            code: Zeroizing::new("493017".to_owned()),
        };
        let options = TransferOptions {
            kdf: KdfParams::DEFAULT,
            ..TransferOptions::default()
        };
        let transfer = Transfer::build(
            &vector_payload(),
            NOW,
            &options,
            &KdfBand::PRODUCTION,
            randomness,
        )
        .expect("build");
        println!("code: {}", transfer.code());
        println!("wrong_code: 493018");
        println!("created_at: {NOW}");
        println!("part_count: {}", transfer.parts().len());
        for part in transfer.parts() {
            println!("part: {part}");
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn any_order_with_repeats_gives_the_payload(
            order in proptest::collection::vec(0usize..64, 0..40),
            identities in 1usize..3,
            servers in proptest::option::of(0u16..12),
            part_bytes in 64usize..700,
        ) {
            let payload = sample(identities, servers);
            let options = TransferOptions { part_bytes, kdf: FAST_KDF, code_digits: 6 };
            let transfer = fast_transfer(&payload, &options);
            let parts = transfer.parts();
            let mut reader = TransferReader::for_tests();
            // A random prefix with repeats, then every part once.
            for index in order {
                reader.add_part(&parts[index % parts.len()], NOW).expect("add part");
            }
            for part in parts {
                reader.add_part(part, NOW).expect("add part");
            }
            prop_assert_eq!(reader.open(transfer.code(), NOW), Ok(payload));
        }

        #[test]
        fn arbitrary_parts_never_panic_and_never_open(
            parts in proptest::collection::vec(
                (any::<[u8; 4]>(), 0u8..6, 1u8..6, proptest::collection::vec(any::<u8>(), 0..120)),
                0..12,
            ),
        ) {
            let mut reader = TransferReader::for_tests();
            for (id, index, total, chunk) in parts {
                let part = Part { id, index, total, chunk };
                let _ = reader.add_part(&part.encode(), NOW);
            }
            prop_assert!(reader.open("123456", NOW).is_err());
        }

        #[test]
        fn a_changed_byte_anywhere_never_yields_another_payload(
            part_index in 0usize..64,
            byte_index in 0usize..4096,
            flip in 1u8..=255,
        ) {
            let payload = sample(1, Some(2));
            let transfer = fast_transfer(&payload, &fast_options());
            let parts = transfer.parts();
            let target = part_index % parts.len();
            let mut reader = TransferReader::for_tests();
            for (index, part) in parts.iter().enumerate() {
                let text = if index == target {
                    forged(part, |part| {
                        let at = byte_index % part.chunk.len();
                        part.chunk[at] ^= flip;
                    })
                } else {
                    part.clone()
                };
                let _ = reader.add_part(&text, NOW);
            }
            prop_assert!(reader.open(transfer.code(), NOW).is_err());
        }
    }

    #[test]
    fn part_overhead_matches_the_documented_layout() {
        assert_eq!(PART_OVERHEAD, 12);
    }
}
