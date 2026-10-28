//! The version the app shows and reports (About, diagnostics, logs, update check), computed at
//! build time. Kept free of I/O so the rules are unit-tested from the library (`src/version.rs`);
//! `build.rs` gathers the inputs from the environment and git.
//!
//! - CI sets `MUMCORD_VERSION`: `X.Y.Z` for a release tag, `dev~<short commit>` otherwise.
//! - Local builds get `dev~YYYYMMDD`, plus `-dirty` when the tree has uncommitted changes. Without
//!   git (for example a source tarball) the suffix is left out instead of failing the build.

/// The display version: a non-empty `override_version` wins, else the dev version for `date`.
/// `dirty` is `None` when git could not tell.
pub fn display_version(
    override_version: Option<&str>,
    dirty: Option<bool>,
    date: (i64, u32, u32),
) -> String {
    if let Some(version) = override_version.map(str::trim).filter(|v| !v.is_empty()) {
        return version.to_owned();
    }
    let (year, month, day) = date;
    let suffix = if dirty == Some(true) { "-dirty" } else { "" };
    format!("dev~{year:04}{month:02}{day:02}{suffix}")
}

/// The UTC calendar date (year, month, day) of a Unix timestamp in seconds.
pub fn civil_date(unix_secs: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-to-civil algorithm; avoids a date crate in the build script.
    let z = unix_secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Both are in range by construction (1..=12 and 1..=31).
    (year, month as u32, day as u32)
}

/// Whether `git status --porcelain` output lists any change.
pub fn is_dirty(porcelain: &str) -> bool {
    porcelain.lines().any(|line| !line.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: (i64, u32, u32) = (2026, 10, 28);

    #[test]
    fn release_tag_version_is_shown_as_is() {
        assert_eq!(display_version(Some("0.4.3"), Some(true), DATE), "0.4.3");
    }

    #[test]
    fn ci_commit_version_is_shown_as_is() {
        assert_eq!(
            display_version(Some("dev~a1b2c3d"), None, DATE),
            "dev~a1b2c3d"
        );
    }

    #[test]
    fn local_build_shows_the_date() {
        assert_eq!(display_version(None, Some(false), DATE), "dev~20261028");
        assert_eq!(
            display_version(Some("  "), Some(false), (2027, 1, 5)),
            "dev~20270105"
        );
    }

    #[test]
    fn uncommitted_changes_mark_the_build_dirty() {
        assert_eq!(
            display_version(None, Some(true), DATE),
            "dev~20261028-dirty"
        );
    }

    #[test]
    fn missing_git_leaves_out_the_dirty_suffix() {
        assert_eq!(display_version(None, None, DATE), "dev~20261028");
    }

    #[test]
    fn unix_time_maps_to_the_utc_date() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(951_782_400), (2000, 2, 29));
        assert_eq!(civil_date(1_793_145_599), (2026, 10, 27));
        assert_eq!(civil_date(1_793_145_600), (2026, 10, 28));
    }

    #[test]
    fn porcelain_output_tells_dirty_from_clean() {
        assert!(!is_dirty(""));
        assert!(!is_dirty("\n"));
        assert!(is_dirty(" M src-tauri/build.rs\n"));
        assert!(is_dirty("?? new-file.txt\n"));
    }
}
