use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use netsim::Netsim;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::timeout;

fn local() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

/// Starts TCP and UDP echo servers on one port and a proxy in front of them.
async fn setup() -> Result<Netsim> {
    let tcp = TcpListener::bind(local()).await?;
    let target = tcp.local_addr()?;
    let udp = UdpSocket::bind(target).await?;
    tokio::spawn(async move {
        loop {
            if let Ok((mut s, _)) = tcp.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = s.read(&mut buf).await {
                        if n == 0 || s.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        }
    });
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = udp.recv_from(&mut buf).await {
            let _ = udp.send_to(&buf[..n], from).await;
        }
    });
    netsim::start(local(), target, local()).await
}

async fn udp_roundtrips(sim: &Netsim, count: usize, wait: Duration) -> Result<usize> {
    let sock = UdpSocket::bind(local()).await?;
    sock.connect(sim.proxy_addr).await?;
    let mut got = 0;
    let mut buf = [0u8; 16];
    for i in 0..count {
        sock.send(&(i as u32).to_be_bytes()).await?;
        if timeout(wait, sock.recv(&mut buf)).await.is_ok() {
            got += 1;
        }
    }
    Ok(got)
}

#[tokio::test]
async fn forwards_tcp_and_udp() -> Result<()> {
    let sim = setup().await?;
    let mut s = TcpStream::connect(sim.proxy_addr).await?;
    s.write_all(b"hello").await?;
    let mut buf = [0u8; 5];
    s.read_exact(&mut buf).await?;
    assert_eq!(&buf, b"hello");
    assert_eq!(udp_roundtrips(&sim, 5, Duration::from_secs(1)).await?, 5);
    Ok(())
}

#[tokio::test]
async fn loss_matches_configured_rate() -> Result<()> {
    let sim = setup().await?;
    // 30% each way -> expected round-trip success 0.7 * 0.7 = 49%.
    netsim::send_command(sim.control_addr, "loss both 30").await?;
    let got = udp_roundtrips(&sim, 600, Duration::from_millis(30)).await?;
    assert!((200..=390).contains(&got), "got {got} of 600");
    Ok(())
}

#[tokio::test]
async fn block_udp_and_latency() -> Result<()> {
    let sim = setup().await?;
    netsim::send_command(sim.control_addr, "block-udp on").await?;
    assert_eq!(
        udp_roundtrips(&sim, 5, Duration::from_millis(100)).await?,
        0
    );
    netsim::send_command(sim.control_addr, "block-udp off").await?;
    netsim::send_command(sim.control_addr, "latency 50").await?;
    let start = std::time::Instant::now();
    assert_eq!(udp_roundtrips(&sim, 1, Duration::from_secs(2)).await?, 1);
    assert!(start.elapsed() >= Duration::from_millis(100));
    let reply = netsim::send_command(sim.control_addr, "bogus").await?;
    assert!(reply.starts_with("err"));
    Ok(())
}

#[tokio::test]
async fn kill_drops_tcp_connections() -> Result<()> {
    let sim = setup().await?;
    let mut s = TcpStream::connect(sim.proxy_addr).await?;
    s.write_all(b"x").await?;
    let mut buf = [0u8; 1];
    s.read_exact(&mut buf).await?;
    netsim::send_command(sim.control_addr, "kill-tcp").await?;
    let n = timeout(Duration::from_secs(2), s.read(&mut buf))
        .await?
        .unwrap_or(0);
    assert_eq!(n, 0);
    // New connections still work.
    let mut s2 = TcpStream::connect(sim.proxy_addr).await?;
    s2.write_all(b"y").await?;
    s2.read_exact(&mut buf).await?;
    assert_eq!(&buf, b"y");
    Ok(())
}
