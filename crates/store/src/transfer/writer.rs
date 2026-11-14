//! The sending side: the desktop turns a selection into QR parts and a typed code.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::{TryRng, rngs::SysRng};
use zeroize::Zeroizing;

use super::envelope::{self, Header, ID_LEN, KdfBand, KdfParams, SALT_LEN};
use super::parts::{MAX_PART_BYTES, PART_OVERHEAD, Part};
use super::payload::TransferPayload;
use super::{
    DEFAULT_CODE_DIGITS, MAX_CODE_DIGITS, MAX_PARTS, MIN_CODE_DIGITS, TransferError, VALIDITY_SECS,
};

/// Bytes per part by default: 600 characters of text, which fit a version 15 QR code with error
/// correction M (77 × 77 modules, about 4 screen pixels per module in a 360 px box). Larger
/// parts mean fewer but denser codes, which a phone reads less reliably from a screen.
pub const DEFAULT_PART_BYTES: usize = 400;

/// Smallest part size; the first part must hold the whole header.
pub const MIN_PART_BYTES: usize = 64;

/// The current time in unix seconds, the `now` of [`Transfer`] and
/// [`TransferReader`](super::TransferReader).
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// How a transfer is built. [`Default`] is what the spec asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferOptions {
    /// Digits of the typed code, [`MIN_CODE_DIGITS`] to [`MAX_CODE_DIGITS`]. Every digit more
    /// makes guessing the code ten times as expensive.
    pub code_digits: u8,
    /// Bytes per part before the text encoding, [`MIN_PART_BYTES`] to [`MAX_PART_BYTES`].
    pub part_bytes: usize,
    /// Cost of the key derivation; must be within what readers accept (64 to 256 MiB, 3 to 8
    /// passes).
    pub kdf: KdfParams,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            code_digits: DEFAULT_CODE_DIGITS,
            part_bytes: DEFAULT_PART_BYTES,
            kdf: KdfParams::DEFAULT,
        }
    }
}

/// A transfer ready to be shown: the texts of the QR codes and the code to type.
///
/// It is valid for [`VALIDITY_SECS`]. When [`seconds_left`](Self::seconds_left) reaches zero
/// the dialog must stop showing it and call [`create`](Self::create) again, which gives new
/// parts and a new code. `Debug` prints neither the code nor the parts.
pub struct Transfer {
    parts: Vec<String>,
    code: Zeroizing<String>,
    created_at: u64,
}

/// The random values of one transfer, separate so that tests can fix them.
pub(crate) struct Randomness {
    pub id: [u8; ID_LEN],
    pub salt: [u8; SALT_LEN],
    pub code: Zeroizing<String>,
}

impl Randomness {
    fn generate(code_digits: u8) -> Result<Self, TransferError> {
        let mut id = [0u8; ID_LEN];
        let mut salt = [0u8; SALT_LEN];
        fill(&mut id)?;
        fill(&mut salt)?;
        Ok(Self {
            id,
            salt,
            code: random_code(code_digits)?,
        })
    }
}

impl Transfer {
    /// Builds a transfer with the default options. `now` is [`unix_now`]. Takes a fraction of a
    /// second (the key derivation), so call it off the UI thread.
    pub fn create(payload: &TransferPayload, now: u64) -> Result<Self, TransferError> {
        Self::create_with(payload, now, &TransferOptions::default())
    }

    pub fn create_with(
        payload: &TransferPayload,
        now: u64,
        options: &TransferOptions,
    ) -> Result<Self, TransferError> {
        if !(MIN_CODE_DIGITS..=MAX_CODE_DIGITS).contains(&options.code_digits) {
            return Err(TransferError::InvalidOptions("code length"));
        }
        let randomness = Randomness::generate(options.code_digits)?;
        Self::build(payload, now, options, &KdfBand::PRODUCTION, randomness)
    }

    pub(crate) fn build(
        payload: &TransferPayload,
        now: u64,
        options: &TransferOptions,
        band: &KdfBand,
        randomness: Randomness,
    ) -> Result<Self, TransferError> {
        if !(MIN_PART_BYTES..=MAX_PART_BYTES).contains(&options.part_bytes) {
            return Err(TransferError::InvalidOptions("part size"));
        }
        if !band.allows(options.kdf) {
            return Err(TransferError::InvalidOptions("key derivation cost"));
        }
        let code_digits = u8::try_from(randomness.code.len())
            .map_err(|_| TransferError::InvalidOptions("code length"))?;
        let compressed = envelope::compress(payload)?;
        let chunk_len = options.part_bytes - PART_OVERHEAD;
        let part_count = envelope::stream_len(compressed.len()).div_ceil(chunk_len);
        if part_count > MAX_PARTS {
            return Err(TransferError::PayloadTooLarge);
        }
        let total = part_count as u8;
        let header = Header {
            id: randomness.id,
            salt: randomness.salt,
            kdf: options.kdf,
            code_digits,
            created_at: now,
            total_parts: total,
            ciphertext_len: (envelope::stream_len(compressed.len()) - envelope::HEADER_LEN) as u32,
        };
        let stream = envelope::seal(header, &randomness.code, compressed)?;
        let parts = stream
            .chunks(chunk_len)
            .zip(0u8..)
            .map(|(chunk, index)| {
                Part {
                    id: header.id,
                    index,
                    total,
                    chunk: chunk.to_vec(),
                }
                .encode()
            })
            .collect();
        Ok(Self {
            parts,
            code: randomness.code,
            created_at: now,
        })
    }

    /// The texts to show as QR codes, one after the other in a loop. Use the QR alphanumeric
    /// mode (the text contains only its 45 characters) and error correction M.
    pub fn parts(&self) -> &[String] {
        &self.parts
    }

    /// The code the user types on the phone, digits only, leading zeros included.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Creation time in unix seconds.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Seconds until the dialog must replace this transfer; zero once it has expired. Never
    /// more than [`VALIDITY_SECS`], also when the clock was set back in the meantime.
    pub fn seconds_left(&self, now: u64) -> u64 {
        self.created_at
            .saturating_add(VALIDITY_SECS)
            .saturating_sub(now)
            .min(VALIDITY_SECS)
    }

    pub fn is_expired(&self, now: u64) -> bool {
        self.seconds_left(now) == 0
    }
}

impl fmt::Debug for Transfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transfer")
            .field("parts", &self.parts.len())
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

fn fill(bytes: &mut [u8]) -> Result<(), TransferError> {
    SysRng
        .try_fill_bytes(bytes)
        .map_err(|_| TransferError::Random)
}

/// A uniformly random code from the operating system's generator. Each digit comes from one
/// random byte; bytes from 250 up are thrown away, because `byte % 10` over all 256 values
/// would make the digits 0 to 5 slightly more likely.
fn random_code(digits: u8) -> Result<Zeroizing<String>, TransferError> {
    let mut code = Zeroizing::new(String::with_capacity(usize::from(digits)));
    let mut byte = Zeroizing::new([0u8; 1]);
    while code.len() < usize::from(digits) {
        fill(&mut byte[..])?;
        if byte[0] < 250 {
            code.push(char::from(b'0' + byte[0] % 10));
        }
    }
    Ok(code)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::envelope::tests::FAST_KDF;
    use super::super::payload::tests::sample;
    use super::*;

    pub(crate) const NOW: u64 = 1_800_000_000;

    pub(crate) fn fast_options() -> TransferOptions {
        TransferOptions {
            kdf: FAST_KDF,
            ..TransferOptions::default()
        }
    }

    /// A transfer with the fast test key derivation and real randomness.
    pub(crate) fn fast_transfer(payload: &TransferPayload, options: &TransferOptions) -> Transfer {
        let randomness = Randomness::generate(options.code_digits).expect("randomness");
        Transfer::build(payload, NOW, options, &KdfBand::TEST, randomness).expect("build")
    }

    #[test]
    fn codes_are_digits_of_the_requested_length() {
        for digits in MIN_CODE_DIGITS..=MAX_CODE_DIGITS {
            let code = random_code(digits).expect("code");
            assert_eq!(code.len(), usize::from(digits));
            assert!(code.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn code_digits_are_evenly_spread() {
        let mut counts = [0u32; 10];
        for _ in 0..2000 {
            for digit in random_code(10).expect("code").bytes() {
                counts[usize::from(digit - b'0')] += 1;
            }
        }
        // 20,000 digits, 2,000 expected each; a standard deviation is about 42.
        for (digit, count) in counts.iter().enumerate() {
            assert!((1700..2300).contains(count), "digit {digit}: {count}");
        }
    }

    #[test]
    fn two_transfers_share_nothing() {
        let payload = sample(1, None);
        let first = fast_transfer(&payload, &fast_options());
        let second = fast_transfer(&payload, &fast_options());
        assert_ne!(first.parts(), second.parts());
        for part in first.parts() {
            assert!(!second.parts().contains(part));
        }
    }

    #[test]
    fn parts_fit_the_chosen_qr_code() {
        let transfer = fast_transfer(&sample(3, Some(20)), &fast_options());
        assert!(transfer.parts().len() > 1);
        for part in transfer.parts() {
            // 600 characters: the alphanumeric capacity of a version 15 code at level M.
            assert!(part.len() <= DEFAULT_PART_BYTES / 2 * 3);
        }
    }

    #[test]
    fn options_are_checked() {
        let payload = sample(1, None);
        let with = |change: &dyn Fn(&mut TransferOptions)| {
            let mut options = TransferOptions::default();
            change(&mut options);
            Transfer::create_with(&payload, NOW, &options).map(|_| ())
        };
        use TransferError::InvalidOptions;
        assert!(matches!(
            with(&|o| o.code_digits = 5),
            Err(InvalidOptions(_))
        ));
        assert!(matches!(
            with(&|o| o.code_digits = 11),
            Err(InvalidOptions(_))
        ));
        assert!(matches!(
            with(&|o| o.part_bytes = 63),
            Err(InvalidOptions(_))
        ));
        assert!(matches!(
            with(&|o| o.part_bytes = 1025),
            Err(InvalidOptions(_))
        ));
        assert!(matches!(
            with(&|o| o.kdf = FAST_KDF),
            Err(InvalidOptions(_))
        ));
        assert!(matches!(
            with(&|o| o.kdf.memory_kib = 512 * 1024),
            Err(InvalidOptions(_))
        ));
    }

    #[test]
    fn a_selection_that_needs_too_many_codes_is_refused() {
        // Keys do not compress: 32 identities of 2 KiB each are far more than 255 small parts.
        let payload = sample(32, None);
        let options = TransferOptions {
            part_bytes: MIN_PART_BYTES,
            ..fast_options()
        };
        let randomness = Randomness::generate(6).expect("randomness");
        assert_eq!(
            Transfer::build(&payload, NOW, &options, &KdfBand::TEST, randomness).err(),
            Some(TransferError::PayloadTooLarge)
        );
    }

    #[test]
    fn the_dialog_helpers_count_down_two_minutes() {
        let transfer = fast_transfer(&sample(1, None), &fast_options());
        assert_eq!(transfer.created_at(), NOW);
        assert_eq!(transfer.seconds_left(NOW), VALIDITY_SECS);
        assert_eq!(transfer.seconds_left(NOW + 119), 1);
        assert!(!transfer.is_expired(NOW + 119));
        assert_eq!(transfer.seconds_left(NOW + 120), 0);
        assert!(transfer.is_expired(NOW + 120));
        assert!(transfer.is_expired(u64::MAX));
        // A desktop clock that was set back does not show more than two minutes.
        assert_eq!(transfer.seconds_left(NOW - 10), VALIDITY_SECS);
    }

    #[test]
    fn debug_output_has_no_code_and_no_parts() {
        let randomness = Randomness {
            id: [1; ID_LEN],
            salt: [2; SALT_LEN],
            code: Zeroizing::new("424242".to_owned()),
        };
        let transfer = Transfer::build(
            &sample(1, None),
            NOW,
            &fast_options(),
            &KdfBand::TEST,
            randomness,
        )
        .expect("build");
        let text = format!("{transfer:?}");
        assert!(!text.contains(transfer.code()));
        assert!(!text.contains(&transfer.parts()[0]));
    }
}
