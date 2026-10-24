use std::net::SocketAddr;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Local TCP/UDP proxy that simulates packet loss, latency and outages")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the proxy until interrupted.
    Run {
        /// Address to listen on (TCP and UDP, same port).
        #[arg(long, default_value = "127.0.0.1:64739")]
        listen: SocketAddr,
        /// Server to forward to.
        #[arg(long, default_value = "127.0.0.1:64738")]
        target: SocketAddr,
        /// Address of the line-based control socket.
        #[arg(long, default_value = "127.0.0.1:64740")]
        control: SocketAddr,
    },
    /// Send a command to a running proxy, e.g. `netsim ctl loss up 20`.
    Ctl {
        #[arg(long, default_value = "127.0.0.1:64740")]
        control: SocketAddr,
        /// loss [up|down|both] <pct> | latency <ms> | jitter <ms> | block-udp on|off | kill-tcp | reset | status
        #[arg(required = true, num_args = 1.., allow_hyphen_values = true)]
        words: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("netsim=info".parse()?),
        )
        .init();
    match Cli::parse().command {
        Command::Run {
            listen,
            target,
            control,
        } => {
            let sim = netsim::start(listen, target, control).await?;
            tracing::info!(
                "proxying {} -> {target}, control on {}",
                sim.proxy_addr,
                sim.control_addr
            );
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        Command::Ctl { control, words } => {
            let reply = netsim::send_command(control, &words.join(" ")).await?;
            println!("{reply}");
            if reply.starts_with("err") {
                bail!("command rejected");
            }
            Ok(())
        }
    }
}
