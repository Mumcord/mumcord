//! Risky file types (FR-7.24): the UI warns before such a file is saved.

use super::sanitize::sanitize_file_name;

const RISKY_EXTENSIONS: [&str; 36] = [
    "exe", "msi", "msix", "msp", "appx", "bat", "cmd", "com", "scr", "pif", "cpl", "hta", "jar",
    "js", "jse", "vbs", "vbe", "wsf", "ps1", "psm1", "reg", "lnk", "url", "app", "dmg", "pkg",
    "command", "sh", "run", "bin", "appimage", "deb", "rpm", "apk", "py", "pl",
];

/// Whether the sanitized `name` has a risky extension. Every extension counts, not just the last
/// one, so `report.exe.pdf` is flagged as well as `report.pdf.exe`.
pub fn is_risky_file_name(name: &str) -> bool {
    let sanitized = sanitize_file_name(name);
    sanitized
        .split('.')
        .skip(1)
        .any(|ext| RISKY_EXTENSIONS.iter().any(|r| ext.eq_ignore_ascii_case(r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases = [
            ("setup.exe", true),
            ("SETUP.EXE", true),
            ("report.pdf.exe", true),
            ("report.exe.pdf", true),
            ("photo\u{202E}gpj.exe", true),
            ("run.sh", true),
            ("app.AppImage", true),
            ("tool.py", true),
            ("notes.txt", false),
            ("archive.tar.gz", false),
            ("exe", false),
            (".exe", false),
            ("report.pdf", false),
            ("script.js.map", true),
            ("trailing.exe. ", true),
        ];
        for (name, risky) in cases {
            assert_eq!(is_risky_file_name(name), risky, "name {name:?}");
        }
    }

    #[test]
    fn every_listed_type_is_flagged() {
        for ext in RISKY_EXTENSIONS {
            assert!(is_risky_file_name(&format!("x.{ext}")), "{ext}");
        }
        assert_eq!(RISKY_EXTENSIONS.len(), 36);
    }
}
