//! The server's ban list (Epic 4, issue #254).
//!
//! A client asks for the list with `BanList { query: true }`; the server answers with the entries
//! it has. Sending the message without `query` replaces the whole list, so Mumcord re-reads it
//! before every save and applies only its own change (FR-10.66).
//!
//! Only `address` + `mask` and `hash` ban anyone; name, reason, start and duration are labels the
//! server keeps for admins. Every text here comes from the server and is untrusted input.

use std::net::IpAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::proto;

/// Length of an address on the wire: IPv4 arrives and goes out IPv4-mapped.
pub const ADDRESS_LEN: usize = 16;

/// One ban entry. `mask` is the subnet length as the server carries it, counted on the 16-byte
/// wire address; [`Ban::mask_bits`] gives the length in the address family it displays as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ban {
    pub address: IpAddr,
    pub mask: u32,
    pub name: Option<String>,
    /// Hex SHA-1 certificate hash; empty or `None` means the ban covers the address only.
    pub hash: Option<String>,
    pub reason: Option<String>,
    /// Start as the server sent it. Mumcord sends the same text back when it saves, so a server
    /// that writes a different shape keeps its own entries intact.
    pub start: Option<String>,
    /// Seconds; 0 means permanent.
    pub duration: u32,
}

impl Ban {
    /// Subnet length in the family `address` shows: 32 for a single IPv4, 128 for a single IPv6.
    /// Servers count the bits on the 16-byte wire address (so an IPv4-mapped /32 arrives as 128),
    /// while clients such as the official client send the family's own length; both read the same.
    pub fn mask_bits(&self) -> u32 {
        match self.address {
            IpAddr::V4(_) if self.mask > 32 => self.mask - 96,
            IpAddr::V4(_) => self.mask,
            IpAddr::V6(_) => self.mask,
        }
    }

    /// Whether the entry bans a certificate at all.
    pub fn bans_hash(&self) -> bool {
        self.hash.as_deref().is_some_and(|h| !h.is_empty())
    }

    /// Start time; `None` when the server sent none or a shape Mumcord cannot read.
    pub fn start_time(&self) -> Option<SystemTime> {
        self.start.as_deref().and_then(parse_iso)
    }

    /// When the ban ends; `None` for a permanent one, and for an unparsable start.
    pub fn end_time(&self) -> Option<SystemTime> {
        if self.duration == 0 {
            return None;
        }
        self.start_time()?
            .checked_add(Duration::from_secs(u64::from(self.duration)))
    }

    pub fn is_expired(&self, now: SystemTime) -> bool {
        self.end_time().is_some_and(|end| end < now)
    }

    /// Key that identifies this entry in the list: the parts the ban acts on. Two entries with
    /// the same key are the same ban, so an edit or a removal can be matched after a re-read.
    pub fn key(&self) -> String {
        format!(
            "{}/{}|{}",
            self.address,
            self.mask,
            self.hash.as_deref().unwrap_or("")
        )
    }
}

/// Reads a server answer.
pub fn from_message(message: &proto::BanList) -> Vec<Ban> {
    message
        .bans
        .iter()
        .filter_map(|e| {
            Some(Ban {
                address: read_address(&e.address)?,
                mask: e.mask,
                name: e.name.clone(),
                hash: e.hash.clone(),
                reason: e.reason.clone(),
                start: e.start.clone(),
                duration: e.duration.unwrap_or(0),
            })
        })
        .collect()
}

/// The query the server answers with the list.
pub fn query_message() -> proto::BanList {
    proto::BanList {
        bans: Vec::new(),
        query: Some(true),
    }
}

/// A full list, which the server takes as its new one.
pub fn to_message(bans: &[Ban]) -> proto::BanList {
    proto::BanList {
        bans: bans.iter().map(entry).collect(),
        query: Some(false),
    }
}

fn entry(ban: &Ban) -> proto::ban_list::BanEntry {
    proto::ban_list::BanEntry {
        address: write_address(ban.address),
        mask: ban.mask,
        name: ban.name.clone(),
        hash: ban.hash.clone(),
        reason: ban.reason.clone(),
        start: ban.start.clone(),
        duration: Some(ban.duration),
    }
}

/// IPv4 is read as IPv4-mapped when the server sends 16 bytes (the protocol's own form), and as
/// 4 bytes on servers that still write it that way.
fn read_address(bytes: &[u8]) -> Option<IpAddr> {
    match bytes {
        &[a, b, c, d] => Some(IpAddr::V4([a, b, c, d].into())),
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, c, d] => {
            Some(IpAddr::V4([a, b, c, d].into()))
        }
        bytes if bytes.len() == ADDRESS_LEN => {
            let bytes: [u8; ADDRESS_LEN] = bytes.try_into().ok()?;
            Some(IpAddr::V6(bytes.into()))
        }
        _ => None,
    }
}

fn write_address(address: IpAddr) -> Vec<u8> {
    match address {
        IpAddr::V6(bytes) => bytes.octets().to_vec(),
        // Always the IPv4-mapped form, as the protocol describes it.
        IpAddr::V4(v4) => {
            let mut out = [0u8; ADDRESS_LEN];
            out[10] = 0xff;
            out[11] = 0xff;
            out[12..].copy_from_slice(&v4.octets());
            out.to_vec()
        }
    }
}

/// Reads a start time. Servers write ISO 8601; murmur has used both the extended form
/// (`2026-11-03T20:21:00.000Z`) and its own compact form (`20261103T202100`), and clients send
/// the extended form without a zone. All are read as UTC.
pub fn parse_iso(text: &str) -> Option<SystemTime> {
    let (date, time) = text.split_once('T')?;
    // Reading the digits and ignoring the separators accepts both shapes without a table of formats.
    let date: String = date.chars().filter(char::is_ascii_digit).collect();
    let time: String = time.chars().filter(char::is_ascii_digit).collect();
    if date.len() != 8 || time.len() < 6 {
        return None;
    }
    let number = |s: &str| s.parse::<u32>().ok();
    let year = date[..4].parse::<i64>().ok()?;
    let (month, day) = (number(&date[4..6])?, number(&date[6..8])?);
    let (hour, minute, second) = (
        number(&time[..2])?,
        number(&time[2..4])?,
        number(&time[4..6])?,
    );
    if month == 0 || month > 12 || day == 0 || day > 31 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let secs = days_from_1970(year, month, day) * 86_400
        + i64::from(hour) * 3600
        + i64::from(minute) * 60
        + i64::from(second);
    UNIX_EPOCH.checked_add(Duration::from_secs(u64::try_from(secs).ok()?))
}

/// Writes a start time for a ban Mumcord adds, in the shape clients send: extended ISO without a
/// zone, which the server reads as UTC.
pub fn format_start(when: SystemTime) -> String {
    let secs = when
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_date(i64::try_from(secs).unwrap_or(0) / 86_400);
    let tod = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z",
        hour = tod / 3600,
        minute = (tod / 60) % 60,
        second = tod % 60
    )
}

/// Civil date from days since 1970-01-01 (H. Hinnant's algorithm), like `src-tauri`'s `rfc3339`.
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn days_from_1970(year: i64, month: u32, day: u32) -> i64 {
    // The same calendar split as `civil_date` above, run the other way (H. Hinnant).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * i64::from(if month > 2 { month - 3 } else { month + 9 }) + 2) / 5
        + i64::from(day)
        - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ipv4_sent_as_ipv4_mapped() {
        let mut bytes = [0u8; ADDRESS_LEN];
        bytes[10] = 0xff;
        bytes[11] = 0xff;
        bytes[12..].copy_from_slice(&[203, 0, 113, 7]);
        assert_eq!(
            read_address(&bytes),
            Some(IpAddr::V4([203, 0, 113, 7].into()))
        );
        assert_eq!(
            read_address(&[198, 51, 100, 9]),
            Some(IpAddr::V4([198, 51, 100, 9].into()))
        );
        assert_eq!(
            read_address(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
            Some(IpAddr::V6(
                [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15].into()
            ))
        );
        assert_eq!(
            read_address(&[1, 2, 3]),
            None,
            "a broken address is skipped"
        );
    }

    #[test]
    fn writes_ipv4_in_the_mapped_form() {
        assert_eq!(
            write_address(IpAddr::V4([203, 0, 113, 7].into()))[12..],
            [203, 0, 113, 7]
        );
    }

    #[test]
    fn mask_is_shown_in_the_address_family() {
        let v4 = Ban {
            address: IpAddr::V4([203, 0, 113, 7].into()),
            mask: 128,
            name: None,
            hash: None,
            reason: None,
            start: None,
            duration: 0,
        };
        assert_eq!(v4.mask_bits(), 32);
        let v6 = Ban {
            address: IpAddr::V6("2001:db8::1".parse().expect("v6")),
            mask: 64,
            name: None,
            hash: None,
            reason: None,
            start: None,
            duration: 0,
        };
        assert_eq!(v6.mask_bits(), 64);
    }

    #[test]
    fn reads_both_start_shapes() {
        assert_eq!(
            parse_iso("2026-11-03T20:21:00.000Z"),
            parse_iso("20261103T202100"),
            "the extended and the compact ISO form are the same time"
        );
        assert_eq!(
            parse_iso("2026-11-03T20:21:00"),
            parse_iso("20261103T202100")
        );
        assert_eq!(parse_iso("2026-11-03 20:21:00"), None);
        assert_eq!(parse_iso("2026-13-03T20:21:00Z"), None);
    }

    #[test]
    fn permanent_bans_have_no_end() {
        let ban = Ban {
            address: IpAddr::V4([198, 51, 100, 9].into()),
            mask: 128,
            name: Some("Mallory".into()),
            hash: Some("a3f9c21e".into()),
            reason: Some("spam links".into()),
            start: Some("2026-11-03T20:21:00Z".into()),
            duration: 0,
        };
        assert_eq!(ban.end_time(), None);
        assert!(!ban.is_expired(SystemTime::now()));
        assert!(ban.bans_hash());
    }

    #[test]
    fn a_round_trip_keeps_the_entry() {
        let ban = Ban {
            address: IpAddr::V6("2001:db8::1".parse().expect("v6")),
            mask: 48,
            name: Some("Mallory".into()),
            hash: Some("a3f9c21e".into()),
            reason: Some("flooding".into()),
            start: Some("2026-11-03T20:21:00Z".into()),
            duration: 7200,
        };
        assert_eq!(
            from_message(&to_message(std::slice::from_ref(&ban)))[0],
            ban
        );
        // The date maths must agree with itself, or a ban Mumcord adds gets a wrong start.
        assert_eq!(
            format_start(parse_iso("2026-11-03T20:21:00Z").expect("parsable")),
            "2026-11-03T20:21:00.000Z"
        );
        assert_eq!(
            format_start(parse_iso("1970-01-01T00:00:00Z").expect("parsable")),
            "1970-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn the_query_is_not_a_list() {
        assert_eq!(query_message().query, Some(true));
        assert_eq!(to_message(&[]).query, Some(false));
    }
}
