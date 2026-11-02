//! Key normalisation and fingerprints for repository keys (FR-6.74).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::verify::RegistryKeys;

/// minisign public key: 2 bytes algorithm (`Ed`), 8 bytes key id, 32 bytes ed25519 key.
const PUBLIC_KEY_BYTES: usize = 42;

/// Returns the bare base64 key line for a minisign public key given either as that line or as a
/// whole `.pub` file, so keys compare equal regardless of how they were pasted. `None` if it is
/// not a minisign public key.
pub fn normalize_key(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))?;
    let bytes = STANDARD.decode(line).ok()?;
    if bytes.len() != PUBLIC_KEY_BYTES || &bytes[..2] != b"Ed" {
        return None;
    }
    RegistryKeys::pinned(line)?;
    Some(line.to_owned())
}

/// The minisign key id as minisign prints it (16 upper-case hex digits, e.g.
/// `1CB49831E26E61AD`). This is the fingerprint the add dialog shows.
pub fn key_fingerprint(key: &str) -> Option<String> {
    Some(format!("{:016X}", key_id(key)?))
}

/// The first 4 bytes of the fingerprint: the source tag third-party plugins carry in announces
/// (FR-6.35 `foo@1~1cb49831`, `messaging::SourceTag::Repo`).
pub fn key_fingerprint_prefix(key: &str) -> Option<[u8; 4]> {
    let id = key_id(key)?.to_be_bytes();
    Some([id[0], id[1], id[2], id[3]])
}

fn key_id(key: &str) -> Option<u64> {
    let line = normalize_key(key)?;
    let bytes = STANDARD.decode(line).ok()?;
    let mut id = [0u8; 8];
    id.copy_from_slice(&bytes[2..10]);
    Some(u64::from_le_bytes(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFICIAL: &str = "RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz";

    #[test]
    fn fingerprint_matches_minisign_key_id() {
        assert_eq!(
            key_fingerprint(OFFICIAL).as_deref(),
            Some("1CB49831E26E61AD")
        );
        assert_eq!(
            key_fingerprint_prefix(OFFICIAL),
            Some([0x1c, 0xb4, 0x98, 0x31])
        );
    }

    #[test]
    fn normalize_accepts_pub_file_and_rejects_junk() {
        let file = format!("untrusted comment: minisign public key 1CB49831E26E61AD\n{OFFICIAL}\n");
        assert_eq!(normalize_key(&file).as_deref(), Some(OFFICIAL));
        assert_eq!(
            normalize_key(&format!("  {OFFICIAL}  ")).as_deref(),
            Some(OFFICIAL)
        );
        assert_eq!(normalize_key("hello"), None);
        assert_eq!(normalize_key(""), None);
        assert_eq!(key_fingerprint("nope"), None);
    }
}
