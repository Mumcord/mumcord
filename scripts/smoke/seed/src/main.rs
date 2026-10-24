//! Seeds a Mumcord profile so the installed app starts straight into the main window and can
//! connect to a local test server without any dialog:
//! - a new identity in the OS secure storage (the same place the wizard puts it),
//! - `wizardCompleted` in the settings,
//! - a saved server `127.0.0.1:<port>` with the given username,
//! - the server's current certificate as trusted (skips the trust-on-first-use dialog).
//!
//! Usage: `mumcord-smoke-seed <app data dir> <port> <username>`
//! Overwrites the stored identity, so only run it on a CI runner or a throwaway profile.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use mumcord_protocol::{ConnectConfig, ConnectError, ServerCert};
use mumcord_store::{Identity, NewServer, SecretStore, Store};

const HOST: &str = "127.0.0.1";

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("seed failed: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data_dir, port, username] = args.as_slice() else {
        return Err("usage: mumcord-smoke-seed <app data dir> <port> <username>".into());
    };
    let data_dir = PathBuf::from(data_dir);
    let port: u16 = port.parse()?;

    let cert = server_cert(port).await?;

    SecretStore::os().save_identity(&Identity::generate(username)?)?;

    std::fs::create_dir_all(&data_dir)?;
    let store = Store::open(&data_dir)?;
    let mut settings = store.settings()?;
    settings.wizard_completed = true;
    store.save_settings(&settings)?;
    let mut server = NewServer::new("Smoke test server", HOST, username);
    server.port = port;
    let saved = store.add_server(&server)?;
    store.set_trusted_cert(saved.id, &fingerprint(&cert.sha256_hex))?;

    println!("seeded server_id={}", saved.id);
    Ok(())
}

/// Reads the server certificate by rejecting it in the TLS handshake.
async fn server_cert(port: u16) -> Result<ServerCert, Box<dyn std::error::Error>> {
    let config = ConnectConfig {
        host: HOST.to_owned(),
        port,
        username: "smoke-seed".to_owned(),
        password: None,
        identity: None,
        verify_cert: Arc::new(|_| false),
        login_timeout: Duration::from_secs(10),
    };
    match mumcord_protocol::connect(config).await {
        Err(ConnectError::CertificateRejected(cert)) => Ok(cert),
        Err(e) => Err(format!("reading the server certificate failed: {e}").into()),
        Ok(_) => Err("server accepted a connection that rejected its certificate".into()),
    }
}

/// Same format the engine stores for a trusted server (`format_fingerprint` in mumcord-engine):
/// upper-case SHA-256 hex pairs separated by colons.
fn fingerprint(sha256_hex: &str) -> String {
    sha256_hex
        .as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}
