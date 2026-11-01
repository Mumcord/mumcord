//! Calendar dates (`YYYY-MM-DD`, UTC) without a date library: registries only need today's date.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub fn today() -> String {
    from_unix_days(unix_seconds() / 86_400)
}

/// Howard Hinnant's `civil_from_days` for days since 1970-01-01.
fn from_unix_days(days: u64) -> String {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `YYYY-MM-DD` with plausible ranges; enough to catch typos in registry files.
pub fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let number = |s: &str, len: usize, max: u32| {
        s.len() == len
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u32>().is_ok_and(|n| (1..=max).contains(&n))
    };
    parts.len() == 3
        && number(parts[0], 4, 9999)
        && number(parts[1], 2, 12)
        && number(parts[2], 2, 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_known_days() {
        assert_eq!(from_unix_days(0), "1970-01-01");
        assert_eq!(from_unix_days(11_016), "2000-02-29");
        assert_eq!(from_unix_days(20_757), "2026-10-31");
        assert!(is_date(&today()));
    }

    #[test]
    fn validates_dates() {
        assert!(is_date("2026-10-01"));
        assert!(!is_date("2026-13-01"));
        assert!(!is_date("2026-1-01"));
        assert!(!is_date("yesterday"));
    }
}
