//! The phone's half of "Send to your phone" for the desktop end-to-end test (spec 05, AC-5.21):
//! reads the QR codes from pictures of the dialog, as a phone camera would, and opens the
//! transfer with the code the dialog shows.
//!
//! Usage: `transfer_read <code> <picture.png>...`
//!
//! Usage: `transfer_read <code> <picture.png>...`, and, in the environment variable `TRANSFER_NEEDLE`, an optional text to look for.
//!
//! Prints one JSON summary and never any secret: identity labels, certificate hashes and default
//! flags, the servers (name, address, port, user name) and `contains_needle`, whether the line
//! given in `TRANSFER_NEEDLE` occurs anywhere in the decoded transfer (labels, certificates, keys, server
//! fields). That lets the test check that nothing else (a server password) came along without the
//! helper ever printing the keys, the code or the payload.

use std::process::ExitCode;

use mumcord_store::cert_hash;
use mumcord_store::transfer::{TransferPayload, TransferReader, unix_now};
use serde_json::json;

fn scan(path: &str) -> Result<String, String> {
    let picture = image::open(path)
        .map_err(|e| format!("{path}: {e}"))?
        .to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(picture);
    let grids = prepared.detect_grids();
    let grid = grids
        .first()
        .ok_or_else(|| format!("{path}: no QR code found"))?;
    let (_, text) = grid.decode().map_err(|e| format!("{path}: {e}"))?;
    Ok(text)
}

fn read(code: &str, pictures: &[String]) -> Result<(), String> {
    let mut reader = TransferReader::new();
    for path in pictures {
        let text = scan(path)?;
        reader
            .add_part(&text, unix_now())
            .map_err(|e| format!("{path}: {e}"))?;
    }
    let payload = reader
        .open(code, unix_now())
        .map_err(|e| format!("opening the transfer: {e}"))?;
    let needle = std::env::var("TRANSFER_NEEDLE").unwrap_or_default();
    let identities: Vec<_> = payload
        .identities
        .iter()
        .map(|identity| {
            json!({
                "label": identity.label,
                "cert_hash": cert_hash(&identity.cert_der),
                "default": identity.is_default,
            })
        })
        .collect();
    let servers: Vec<_> = payload
        .servers
        .iter()
        .flatten()
        .map(|server| {
            json!({
                "name": server.name,
                "address": server.address,
                "port": server.port,
                "username": server.username,
            })
        })
        .collect();
    let summary = json!({
        "identities": identities,
        "servers": servers,
        "contains_needle": !needle.is_empty() && contains(&payload, needle.as_bytes()),
    });
    println!("{summary}");
    Ok(())
}

fn contains(payload: &TransferPayload, needle: &[u8]) -> bool {
    let has = |bytes: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    payload.identities.iter().any(|identity| {
        has(identity.label.as_bytes()) || has(&identity.cert_der) || has(identity.key_pkcs8_der())
    }) || payload.servers.iter().flatten().any(|server| {
        has(server.name.as_bytes())
            || has(server.address.as_bytes())
            || has(server.username.as_bytes())
            || server
                .identity_fingerprint
                .as_deref()
                .is_some_and(|f| has(f.as_bytes()))
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((code, pictures)) = args.split_first() else {
        eprintln!("usage: transfer_read <code> <picture.png>...");
        return ExitCode::from(2);
    };
    match read(code, pictures) {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("FAILED: {problem}");
            ExitCode::FAILURE
        }
    }
}
