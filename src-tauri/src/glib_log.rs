//! GLib, GTK, GDK and WebKit log messages in Mumcord's own log (issue #625).
//!
//! These libraries write their warnings to stderr only, so neither the log file nor a crash
//! report saw them. On Wayland GTK ends the process with `_exit` right after its message when the
//! display connection fails ("Error 71 (Protocol error) dispatching to Wayland display"); such a
//! run leaves no signal and no backtrace, and the message is the only clue. Mumcord's log writes
//! every line straight to disk, so the line is there for the next start's crash report.
//!
//! Message, Warning, Critical and Error go to the log; Info and Debug stay out so the log is not
//! flooded. Every line still goes to stderr as before, and through the log's usual redaction.

use gtk::glib::{self, LogField, LogLevel, LogWriterOutput};

/// Routes GLib's log through Mumcord's. Called once, before GTK and WebKit start.
pub fn install() {
    glib::log_set_writer_func(write);
}

fn write(level: LogLevel, fields: &[LogField<'_>]) -> LogWriterOutput {
    if let Some(level) = level_of(level) {
        let domain = field(fields, "GLIB_DOMAIN").unwrap_or("GLib");
        let message = field(fields, "MESSAGE").unwrap_or_default();
        match level {
            tracing::Level::ERROR => tracing::error!(target: "glib", domain, "{message}"),
            tracing::Level::WARN => tracing::warn!(target: "glib", domain, "{message}"),
            _ => tracing::info!(target: "glib", domain, "{message}"),
        }
    }
    glib::log_writer_default(level, fields)
}

/// The level a GLib message gets in Mumcord's log; `None` for the chatty ones.
fn level_of(level: LogLevel) -> Option<tracing::Level> {
    match level {
        LogLevel::Error | LogLevel::Critical => Some(tracing::Level::ERROR),
        LogLevel::Warning => Some(tracing::Level::WARN),
        LogLevel::Message => Some(tracing::Level::INFO),
        LogLevel::Info | LogLevel::Debug => None,
    }
}

fn field<'a>(fields: &'a [LogField<'_>], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|f| f.key() == key)
        .and_then(LogField::value_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib::GStr;

    #[test]
    fn warnings_and_worse_reach_the_log_and_chatter_stays_out() {
        assert_eq!(level_of(LogLevel::Critical), Some(tracing::Level::ERROR));
        assert_eq!(level_of(LogLevel::Warning), Some(tracing::Level::WARN));
        // GDK announces the fatal Wayland error as a message.
        assert_eq!(level_of(LogLevel::Message), Some(tracing::Level::INFO));
        assert_eq!(level_of(LogLevel::Debug), None);
        assert_eq!(level_of(LogLevel::Info), None);
    }

    #[test]
    fn reads_domain_and_message_fields() {
        let domain = LogField::new(
            GStr::from_str_with_nul("GLIB_DOMAIN\0").expect("key"),
            b"Gdk",
        );
        let text = b"Error 71 (Protocol error) dispatching to Wayland display.";
        let message = LogField::new(GStr::from_str_with_nul("MESSAGE\0").expect("key"), text);
        let fields = [domain, message];
        assert_eq!(field(&fields, "GLIB_DOMAIN"), Some("Gdk"));
        assert_eq!(
            field(&fields, "MESSAGE"),
            Some("Error 71 (Protocol error) dispatching to Wayland display.")
        );
        assert_eq!(field(&fields, "CODE_FILE"), None);
    }

    /// End to end: a GTK-style message emitted through GLib lands in Mumcord's log, with its
    /// domain, as the next crash report would read it.
    #[test]
    fn a_glib_message_lands_in_mumcords_log() {
        let dir = std::env::temp_dir().join(format!("mumcord-glib-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("log dir");
        mumcord_engine::diag::init_early().expect("logging");
        mumcord_engine::diag::attach_file(&dir, mumcord_engine::diag::LogLevel::Info, 2)
            .expect("log file");
        install();
        glib::g_message!(
            "Gdk",
            "Error 71 (Protocol error) dispatching to Wayland display."
        );
        glib::g_debug!("Gdk", "chatty debug line");
        let lines = mumcord_engine::diag::recent_log_lines();
        let line = lines
            .iter()
            .find(|l| l.contains("dispatching to Wayland display"))
            .expect("the message is in the log");
        assert!(line.contains("glib") && line.contains("Gdk"), "{line}");
        assert!(!lines.iter().any(|l| l.contains("chatty debug line")));
        // Log writes are buffered (spec 12); flush before reading the file.
        mumcord_engine::diag::flush_log();
        let on_disk: String = std::fs::read_dir(&dir)
            .expect("log dir")
            .filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok())
            .collect();
        assert!(
            on_disk.contains("dispatching to Wayland display"),
            "written to disk after the flush"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
