//! Local TCP+UDP forwarding proxy that can degrade the UDP path and cut TCP
//! connections at runtime. Pure userspace: no root, no iptables.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rand::RngExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::watch;

const MAX_DATAGRAM: usize = 65_536;
/// A UDP client session is dropped after this long without upstream traffic.
const UDP_SESSION_IDLE: Duration = Duration::from_secs(120);

/// Runtime-adjustable impairments, shared by all proxy tasks.
#[derive(Debug)]
pub struct Controls {
    /// Loss in hundredths of a percent, client -> server.
    loss_up: AtomicU32,
    /// Loss in hundredths of a percent, server -> client.
    loss_down: AtomicU32,
    latency_ms: AtomicU32,
    jitter_ms: AtomicU32,
    udp_blocked: AtomicBool,
    kill_epoch: watch::Sender<u64>,
}

impl Controls {
    fn new() -> Self {
        Self {
            loss_up: AtomicU32::new(0),
            loss_down: AtomicU32::new(0),
            latency_ms: AtomicU32::new(0),
            jitter_ms: AtomicU32::new(0),
            udp_blocked: AtomicBool::new(false),
            kill_epoch: watch::channel(0).0,
        }
    }

    fn set_loss(&self, dir: &str, pct: f64) -> Result<()> {
        if !(0.0..=100.0).contains(&pct) {
            bail!("loss must be between 0 and 100");
        }
        let hundredths = (pct * 100.0).round() as u32;
        match dir {
            "up" => self.loss_up.store(hundredths, Ordering::Relaxed),
            "down" => self.loss_down.store(hundredths, Ordering::Relaxed),
            "both" => {
                self.loss_up.store(hundredths, Ordering::Relaxed);
                self.loss_down.store(hundredths, Ordering::Relaxed);
            }
            _ => bail!("direction must be up, down or both"),
        }
        Ok(())
    }

    /// Cuts every currently open TCP connection. New ones are still accepted.
    pub fn kill_tcp(&self) {
        self.kill_epoch.send_modify(|e| *e += 1);
    }

    fn drops(&self, loss: &AtomicU32) -> bool {
        let hundredths = loss.load(Ordering::Relaxed);
        hundredths > 0 && rand::rng().random_range(0..10_000) < hundredths
    }

    fn delay(&self) -> Duration {
        let jitter = self.jitter_ms.load(Ordering::Relaxed);
        let extra = if jitter > 0 {
            rand::rng().random_range(0..=jitter)
        } else {
            0
        };
        Duration::from_millis(u64::from(self.latency_ms.load(Ordering::Relaxed) + extra))
    }

    fn status(&self) -> String {
        format!(
            "loss-up={} loss-down={} latency={} jitter={} block-udp={}",
            f64::from(self.loss_up.load(Ordering::Relaxed)) / 100.0,
            f64::from(self.loss_down.load(Ordering::Relaxed)) / 100.0,
            self.latency_ms.load(Ordering::Relaxed),
            self.jitter_ms.load(Ordering::Relaxed),
            self.udp_blocked.load(Ordering::Relaxed),
        )
    }

    /// Applies one control line and returns the reply text (without "ok"/"err").
    fn apply(&self, line: &str) -> Result<String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["loss", dir, pct] => self.set_loss(dir, pct.parse().context("invalid percentage")?)?,
            ["loss", pct] => self.set_loss("both", pct.parse().context("invalid percentage")?)?,
            ["latency", ms] => self.latency_ms.store(
                ms.parse().context("invalid milliseconds")?,
                Ordering::Relaxed,
            ),
            ["jitter", ms] => self.jitter_ms.store(
                ms.parse().context("invalid milliseconds")?,
                Ordering::Relaxed,
            ),
            ["block-udp", "on"] => self.udp_blocked.store(true, Ordering::Relaxed),
            ["block-udp", "off"] => self.udp_blocked.store(false, Ordering::Relaxed),
            ["kill-tcp"] => self.kill_tcp(),
            ["reset"] => {
                self.set_loss("both", 0.0)?;
                self.latency_ms.store(0, Ordering::Relaxed);
                self.jitter_ms.store(0, Ordering::Relaxed);
                self.udp_blocked.store(false, Ordering::Relaxed);
            }
            ["status"] => {}
            _ => bail!(
                "unknown command; use: loss [up|down|both] <pct> | latency <ms> | jitter <ms> | \
                 block-udp on|off | kill-tcp | reset | status"
            ),
        }
        Ok(self.status())
    }
}

/// A running proxy. Dropping it does not stop the tasks; they end with the runtime.
#[derive(Debug)]
pub struct Netsim {
    pub proxy_addr: SocketAddr,
    pub control_addr: SocketAddr,
    pub controls: Arc<Controls>,
}

/// Starts the proxy on `listen` (TCP and UDP, same port) and the control socket on `control`.
/// A port of 0 in `listen` picks a free port that is valid for both protocols.
pub async fn start(listen: SocketAddr, target: SocketAddr, control: SocketAddr) -> Result<Netsim> {
    let controls = Arc::new(Controls::new());
    let tcp = TcpListener::bind(listen)
        .await
        .context("bind TCP listener")?;
    let proxy_addr = tcp.local_addr()?;
    let udp = Arc::new(
        UdpSocket::bind(proxy_addr)
            .await
            .context("bind UDP socket")?,
    );
    let ctl = TcpListener::bind(control)
        .await
        .context("bind control socket")?;
    let control_addr = ctl.local_addr()?;

    tokio::spawn(tcp_accept_loop(tcp, target, controls.clone()));
    tokio::spawn(udp_loop(udp, target, controls.clone()));
    tokio::spawn(control_loop(ctl, controls.clone()));
    Ok(Netsim {
        proxy_addr,
        control_addr,
        controls,
    })
}

async fn tcp_accept_loop(listener: TcpListener, target: SocketAddr, controls: Arc<Controls>) {
    loop {
        let (client, peer) = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("TCP accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let mut kill = controls.kill_epoch.subscribe();
        tokio::spawn(async move {
            let result = tokio::select! {
                r = tcp_forward(client, target) => r,
                _ = kill.changed() => {
                    tracing::info!("TCP connection from {peer} killed");
                    return;
                }
            };
            if let Err(e) = result {
                tracing::debug!("TCP connection from {peer} ended: {e}");
            }
        });
    }
}

async fn tcp_forward(mut client: TcpStream, target: SocketAddr) -> Result<()> {
    let mut server = TcpStream::connect(target)
        .await
        .context("connect to target")?;
    tokio::io::copy_bidirectional(&mut client, &mut server).await?;
    Ok(())
}

type Sessions = Arc<Mutex<HashMap<SocketAddr, Arc<UdpSocket>>>>;

/// Sends `data` after the configured delay without blocking the caller or reordering
/// more than the jitter implies.
fn send_later(delay: Duration, data: Vec<u8>, socket: Arc<UdpSocket>, to: Option<SocketAddr>) {
    tokio::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let result = match to {
            Some(addr) => socket.send_to(&data, addr).await,
            None => socket.send(&data).await,
        };
        if let Err(e) = result {
            tracing::debug!("UDP send failed: {e}");
        }
    });
}

async fn udp_loop(socket: Arc<UdpSocket>, target: SocketAddr, controls: Arc<Controls>) {
    let sessions: Sessions = Arc::default();
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        let (len, client) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("UDP recv failed: {e}");
                continue;
            }
        };
        if controls.udp_blocked.load(Ordering::Relaxed) || controls.drops(&controls.loss_up) {
            continue;
        }
        let existing = sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&client)
            .cloned();
        let upstream = match existing {
            Some(u) => u,
            None => match new_session(&socket, target, client, &controls, &sessions).await {
                Ok(u) => u,
                Err(e) => {
                    tracing::error!("UDP session for {client} failed: {e:#}");
                    continue;
                }
            },
        };
        send_later(controls.delay(), buf[..len].to_vec(), upstream, None);
    }
}

async fn new_session(
    front: &Arc<UdpSocket>,
    target: SocketAddr,
    client: SocketAddr,
    controls: &Arc<Controls>,
    sessions: &Sessions,
) -> Result<Arc<UdpSocket>> {
    let bind: SocketAddr = if target.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    }
    .parse()?;
    let upstream = Arc::new(UdpSocket::bind(bind).await?);
    upstream.connect(target).await?;
    sessions
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(client, upstream.clone());

    let (front, controls, sessions, up) = (
        front.clone(),
        controls.clone(),
        sessions.clone(),
        upstream.clone(),
    );
    tokio::spawn(async move {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        loop {
            let len = match tokio::time::timeout(UDP_SESSION_IDLE, up.recv(&mut buf)).await {
                Ok(Ok(len)) => len,
                Ok(Err(e)) => {
                    tracing::debug!("UDP upstream recv for {client} failed: {e}");
                    continue;
                }
                Err(_) => break,
            };
            if controls.udp_blocked.load(Ordering::Relaxed) || controls.drops(&controls.loss_down) {
                continue;
            }
            send_later(
                controls.delay(),
                buf[..len].to_vec(),
                front.clone(),
                Some(client),
            );
        }
        sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&client);
    });
    Ok(upstream)
}

async fn control_loop(listener: TcpListener, controls: Arc<Controls>) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("control accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let controls = controls.clone();
        tokio::spawn(async move {
            if let Err(e) = control_session(stream, &controls).await {
                tracing::debug!("control session ended: {e}");
            }
        });
    }
}

async fn control_session(stream: TcpStream, controls: &Controls) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let reply = match controls.apply(&line) {
            Ok(text) => format!("ok {text}\n"),
            Err(e) => format!("err {e:#}\n"),
        };
        write.write_all(reply.as_bytes()).await?;
    }
    Ok(())
}

/// Sends one command line to a running proxy's control socket and returns its reply.
pub async fn send_command(control: SocketAddr, command: &str) -> Result<String> {
    let stream = TcpStream::connect(control)
        .await
        .context("connect to control socket")?;
    let (read, mut write) = stream.into_split();
    write.write_all(format!("{command}\n").as_bytes()).await?;
    let reply = BufReader::new(read)
        .lines()
        .next_line()
        .await?
        .context("control socket closed without a reply")?;
    Ok(reply)
}
