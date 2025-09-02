// src/bin/anc.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use bytes::Bytes;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};
use std::{io, net::SocketAddr, path::PathBuf};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, task};

use aranya_util::Addr;
use aranya_client::Client;
use aranya_client::aqc::{AqcBidiChannel, AqcBidiStream};
use aranya_daemon_api::{LabelId, NetIdentifier, TeamId};


#[derive(Parser)]
struct Common {
    /// Path to this device's aranya-daemon UDS socket
    #[arg(long, value_name = "SOCKPATH")]
    daemon_sock: PathBuf,
    /// Port for the AQC server on localhost
    #[arg(long, value_name = "PORT")]
    aqc_port: u16,
}

#[derive(Parser)]
struct Args {
   #[command(flatten)]
   common: Common,
   #[command(subcommand)]
   cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Wait for a peer to open a bidi channel and then bridge stdin/stdout
    Listen { team_id: String, label_id: String },
    /// Open a bidi channel to peer (host:port or dns:port) and bridge stdin/stdout
    Dial { team_id: String, label_id: String, peer: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    // added to enable logging
    init_tracing_minimal();

    let args = Args::parse();

    // construct AQC server addr from port argument
    let aqc_server_addr = Addr::from(([127, 0, 0, 1], args.common.aqc_port));

    // 1) connect client to local daemon
    // (use ClientBuilder setters if you need a custom UDS path, see docs)
    let client = Client::builder()
        .daemon_uds_path(&args.common.daemon_sock)
        .aqc_server_addr(&aqc_server_addr)
        .connect()
        .await
        .context("connecting to daemon")?;
    info!("Connected to aranya-daemon");

    match args.cmd {
        Cmd::Listen { team_id, label_id } => {
            let team_id: TeamId = team_id.parse()?;   // TeamId
            let label_id: LabelId = label_id.parse()?; // LabelId

            let aqc = client.aqc(); // get AQC handle
            info!("Listening on {:?}", aqc.server_addr()); // doc: server_addr()
            // Wait for peer to create a channel with us:
            let ch = match aqc.receive_channel().await? {
                aranya_client::aqc::AqcPeerChannel::Bidi(ch) => ch,
                _ => anyhow::bail!("expected bidi channel"),
            };
            run_netcat(ch, false, label_id).await?;
        }
        Cmd::Dial { team_id, label_id, peer } => {
            info!("Dialing peer {}", peer);
            let team_id = team_id.parse()?;   // TeamId
            let label_id = label_id.parse()?; // LabelId

            // Build the peer NetIdentifier. The AQC APIs accept DNS or IPv4 + port.
            let peer_sock: SocketAddr = peer.parse::<SocketAddr>()?;
            let mut aqc = client.aqc();
            let net_id = NetIdentifier(
                peer_sock
                .to_string()
                .try_into()
                .expect("address is valid text")
            ); // trying to convert SocketAddres to NetIdentifier like in example

            // Create a bidi channel to the peer, authorized by the label.
            let ch = aqc.create_bidi_channel(team_id, net_id, label_id).await?;
        
            // Run netcat bridge
            run_netcat(ch, true, label_id).await?;
        }
    }
    Ok(())
}

async fn run_netcat(mut ch: AqcBidiChannel, dialer_makes_stream: bool, _label: impl std::fmt::Debug) -> Result<()> {
    // What “channel vs stream” means:
    // - A channel is the session between two devices (authorized by the label).
    // - Within a channel you can create multiple streams; we use one bidi stream like netcat.
    //   (See AqcBidiChannel::{create_bidi_stream,receive_stream}).

    let stream: AqcBidiStream = if dialer_makes_stream {
        ch.create_bidi_stream().await.context("create_bidi_stream")?
    } else {
        // Wait for the peer's first stream
        match ch.receive_stream().await.context("receive_stream")? {
            aranya_client::aqc::AqcPeerStream::Bidi(s) => s,
            _ => anyhow::bail!("peer opened uni stream; expected bidi"),
        }
    };

    // Split the stream into read and write halves
    let (mut send_half, mut receive_half) = stream.split();

    // Pipe stdin -> send   and   recv -> stdout
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    // Reader task: from AQC -> stdout
    let recv_task = task::spawn(async move {
        info!("Starting receive loop...");
        loop {
            // The bidi stream lets you receive messages and write them to stdout.
            let data = match receive_half
                .receive()
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))? {
                    Some(data) if data.is_empty() => break,
                    Some(data) => data,
                    None => break,
                };
            info!("Received {} bytes", data.len());
            stdout.write_all(&data).await?;
            stdout.flush().await?;
        }
        io::Result::Ok(())
    });

    // Writer loop: stdin -> AQC
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        info!("Waiting for stdin...");
        let n = stdin.read(&mut buf).await?;
        if n == 0 {
            // EOF -> close gracefully
            ch.close(); // close channel (docs: AqcBidiChannel::close)
            info!("EOF on stdin, closing AQC channel...");
            break;
        }
        // Send the chunk. AqcBidiStream uses bytes::Bytes.
        info!("Sending {} bytes", n);
        send_half.send(Bytes::copy_from_slice(&buf[..n])).await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        info!("Sent {} bytes", n);
    }

    let _ = recv_task.await;
    Ok(())
}

/// A: Minimal — prints INFO+ by default (no env var required)
fn init_tracing_minimal() {
    tracing_subscriber::fmt().init();
}

/// B: Honor RUST_LOG, but fall back to INFO when RUST_LOG is not set
fn init_tracing_with_env() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}
