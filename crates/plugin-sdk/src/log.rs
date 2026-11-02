//! Plugin log (shown in Diagnostics and the Developer tab). Never log chat text, peer payloads,
//! secrets or other personal data.

pub use crate::bindings::mumcord::plugin::log::*;

pub fn debug(message: &str) {
    write(Level::Debug, message);
}

pub fn info(message: &str) {
    write(Level::Info, message);
}

pub fn warn(message: &str) {
    write(Level::Warn, message);
}

pub fn error(message: &str) {
    write(Level::Error, message);
}
