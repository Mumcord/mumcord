//! FreeBSD's packaging smoke observer. The pinned mumbot tester's published protocol dependency
//! uses a vendored protoc that does not support FreeBSD, so use the workspace protocol instead.
//! Connects only to a local test server and succeeds when the expected user appears.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use mumcord_protocol::{ConnectConfig, ConnectError, Event};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("smoke observer failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [port, username, timeout] = args.as_slice() else {
        return Err(
            "usage: mumcord-smoke-observe <port> <expected username> <timeout seconds>".into(),
        );
    };
    tokio::time::timeout(
        Duration::from_secs(timeout.parse()?),
        expect_user(port.parse()?, username),
    )
    .await
    .map_err(|_| format!("timed out waiting for {username} on the local server"))?
}

fn config(port: u16) -> ConnectConfig {
    ConnectConfig {
        host: "127.0.0.1".to_owned(),
        port,
        username: "SmokeObserver".to_owned(),
        password: None,
        tokens: Vec::new(),
        identity: None,
        verify_cert: Arc::new(|_| false),
        login_timeout: Duration::from_secs(10),
    }
}

async fn expect_user(port: u16, username: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Pin the loopback server's certificate, just as the profile seeder does. Never disable TLS
    // verification globally or allow this helper to connect to a non-local server.
    let cert = match mumcord_protocol::connect(config(port)).await {
        Err(ConnectError::CertificateRejected(cert)) => cert,
        Err(error) => return Err(error.into()),
        Ok(_) => return Err("server accepted a rejected certificate".into()),
    };
    let mut cfg = config(port);
    cfg.verify_cert = Arc::new(move |seen| seen.sha256_hex == cert.sha256_hex);
    let mut connection = mumcord_protocol::connect(cfg).await?;
    loop {
        // Also covers users already present in the initial ServerSync snapshot.
        if connection
            .handle
            .with_state(|state| state.users.values().any(|user| user.name == username))
        {
            connection.handle.disconnect();
            println!("local server sees {username}");
            return Ok(());
        }
        match connection.events.recv().await {
            Some(Event::Disconnected(reason)) => {
                return Err(format!("observer disconnected: {reason:?}").into());
            }
            None => return Err("observer event stream closed".into()),
            Some(_) => {}
        }
    }
}
