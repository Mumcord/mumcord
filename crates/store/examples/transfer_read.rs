//! The phone's half of "Send to your phone" for the desktop end-to-end test (spec 05, AC-5.21):
//! reads the QR codes from pictures of the dialog, as a phone camera would, and opens the
//! transfer with the code the dialog shows.
//!
//! Usage: `transfer_read <code> <picture.png>...`
//!
//! Prints one line per entry of the transfer, fields separated by tabs:
//! `identity <label> <certificate hash> <default|->` and
//! `server <name> <address> <port> <user name>`. That is everything a transfer holds besides the
//! keys, so the test can check that nothing else (a server password) came along.

use std::process::ExitCode;

use mumcord_store::cert_hash;
use mumcord_store::transfer::{TransferReader, unix_now};

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
    for identity in &payload.identities {
        println!(
            "identity\t{}\t{}\t{}",
            identity.label,
            cert_hash(&identity.cert_der),
            if identity.is_default { "default" } else { "-" }
        );
    }
    for server in payload.servers.iter().flatten() {
        println!(
            "server\t{}\t{}\t{}\t{}",
            server.name, server.address, server.port, server.username
        );
    }
    Ok(())
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
