// src/owner.rs
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{debug, info};
use tracing_subscriber::{EnvFilter, fmt};
use std::{fs, net::SocketAddr, path::PathBuf, time::Duration};

use aranya_util::Addr;
use aranya_client::{
    AddTeamConfig, AddTeamConfigBuilder, AddTeamQuicSyncConfig, Client, CreateTeamConfig, CreateTeamQuicSyncConfig, SyncPeerConfig, SyncPeerConfigBuilder
};
use aranya_daemon_api::{text, TeamId, ChanOp, NetIdentifier};

#[derive(Parser)]
struct Common {
    /// Path to this device's aranya-daemon UDS socket
    #[arg(long, value_name = "SOCKPATH")]
    daemon_sock: PathBuf,
    /// Port for the AQC server on localhost
    #[arg(long, value_name = "PORT", default_value = "50000")]
    aqc_port: u16,
    /// Path to store/load the team seed (default: ./team_seed.bin)
    #[arg(long, value_name = "SEEDPATH", default_value = "/Users/matcala/Desktop/internship/aranya/aranya/examples/rust/aranya-netcat/team_seed.bin")]
    seed_file: PathBuf,
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
    /// Create a new team and print its TeamId (base58) to stdout
    CreateTeam,
    /// Join an existing team by TeamId (base58)
    AddTeam { team_id: String },
    /// Export this device's public key bundle to a file
    ExportKeys { out: PathBuf },
    /// Add another device to the team from a key bundle file
    AddDevice { team_id: String, key_bundle_file: PathBuf },
    /// Create a label (e.g. "chat") and print its LabelId
    CreateLabel { team_id: String, name: String },
    /// Grant label send/recv to a device (by its device id string)
    GrantLabel { team_id: String, device_id: String, label_id: String, op: String /* "Send" or "Recv" or "Bidi" */ },
    /// Assign the AQC net identifier (host:port) to a device
    SetNetId { team_id: String, device_id: String, host_port: String },
    /// Add a sync peer for this team (host:port of peer daemon)
    AddSyncPeer { team_id: String, host_port: String },
    /// Print this device's ID
    GetDeviceId,
    /// Print this device's AQC server listening address
    GetAqcAddr,
    /// Force sync with a specific peer or all peers
    SyncNow { team_id: String, peer_addr: Option<String> },
    /// Query and print team diagnostics from fact database
    QueryTeam { team_id: String },
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
    info!("connected to aranya-daemon");

    match args.cmd {
        Cmd::CreateTeam => {
            // Generate new seed and save it
            let seed_ikm = generate_and_save_seed(&client, &args.common.seed_file).await?;
            
            let qs_cfg = CreateTeamQuicSyncConfig::builder()
                .seed_ikm(seed_ikm)
                .build()?;
            let cfg = CreateTeamConfig::builder()
                .quic_sync(qs_cfg)
                .build()?;
            let team = client.create_team(cfg).await?;
            info!("Created team with id: {}", team.team_id());
            info!("Seed saved to: {}", args.common.seed_file.display());
        }
        Cmd::AddTeam { team_id } => {
            // Load existing seed
            let seed_ikm = load_seed(&args.common.seed_file)?;
            info!("Loaded existing team seed from: {}", args.common.seed_file.display());
            let qs_cfg = AddTeamQuicSyncConfig::builder()
                .seed_ikm(seed_ikm)
                .build()?;

            let cfg = AddTeamConfig::builder()
                .team_id(team_id.parse()?)
                .quic_sync(qs_cfg)
                .build()?;
            let team = client.add_team(cfg).await?;
            info!("Added team {}", team.team_id());
        }
        Cmd::ExportKeys { out } => {
            let kb = client.get_key_bundle().await?; // shown in aranya-client tests
            fs::write(&out, bincode::serialize(&kb)?)?;  // fixed: leveraging serde macro for serialization/deserialization
            info!("Exported key bundle to file: {}", out.display());
        }
        Cmd::AddDevice { team_id, key_bundle_file } => {
            let data = fs::read(key_bundle_file)?;
            let kb = bincode::deserialize(&data)?; // fixed: leveraging serde macro for serialization/deserialization
            let team = client.team(parse_team(&team_id)?);
            team.add_device_to_team(kb).await?;
            //TODO: fix so that it prints the added device's ID, not this device's ID
            info!("Added device {} to team {}", client.get_device_id().await? ,team.team_id());
        }
        Cmd::CreateLabel { team_id, name } => {
            let team = client.team(parse_team(&team_id)?);
            let label_id = team.create_label(text!(stringify!(name))).await?; //added stringify because macro wanted string literal
            info!("Created AQC label with id: {}", label_id);
        }
        Cmd::GrantLabel { team_id, device_id, label_id, op } => {
            let team = client.team(parse_team(&team_id)?);
            let device = device_id.parse()?; // type: DeviceId
            let label = label_id.parse()?;   // type: LabelId
            let chan_op = match op.as_str() {
                "send" => ChanOp::SendOnly,
                "recv" => ChanOp::RecvOnly,
                "bidi" => ChanOp::SendRecv,
                _ => anyhow::bail!("op must be Send|Recv|Bidi"),
            };
            team.assign_label(device, label, chan_op).await?;
            info!("Granted label {} {:?} to device {}", label, chan_op, device);
        }
        Cmd::SetNetId { team_id, device_id, host_port } => {
            let team = client.team(parse_team(&team_id)?);
            let device = device_id.parse()?; // DeviceId
            let addr: SocketAddr = host_port.parse()?; // "1.2.3.4:4444" or "name:4444"
            let net_id = NetIdentifier(
                addr
                .to_string()
                .try_into()
                .expect("address is valid text")
            ); // trying to convert SocketAddres to NetIdentifier like in example
            team.assign_aqc_net_identifier(device, net_id.clone()).await?;
            info!("Assigned net id {} to device {}", net_id.0, device);
        }
        Cmd::AddSyncPeer { team_id, host_port } => {
            let team = client.team(parse_team(&team_id)?);
            let addr: std::net::SocketAddr = host_port.parse()?;

            // borrowed from example
            // sync peer config with interval required
            let sync_interval = Duration::from_millis(100);
            // let sleep_interval = sync_interval * 6;
            let sync_cfg = SyncPeerConfig::builder().interval(sync_interval).build()?;

            team.add_sync_peer(addr.into(), sync_cfg).await?;
            info!("Added sync peer {} to team {}", addr, team.team_id());

            info!("Syncing now...");
            team.sync_now(addr.into(), None).await?;
        }
        Cmd::GetDeviceId => {
            let device_id = client.get_device_id().await?;
            println!("This device's ID is: {}", device_id);
        }
        Cmd::GetAqcAddr => {
            let aqc_addr = client.aqc().server_addr();
            println!("AQC server listening on: {}", aqc_addr);
        }
        Cmd::SyncNow { team_id, peer_addr } => {
            let team = client.team(parse_team(&team_id)?);
            
            if let Some(addr_str) = peer_addr {
                // Sync with specific peer
                let addr: SocketAddr = addr_str.parse()?;
                info!("Syncing with peer {} now...", addr);
                team.sync_now(addr.into(), None).await?;
                info!("Sync with peer {} completed", addr);
            } else {
                // If no specific peer provided, we can't sync with "all peers" 
                // since sync_now requires a specific address
                anyhow::bail!("Peer address is required for sync. Use: sync-now <team_id> <peer_addr>");
            }
        }
        Cmd::QueryTeam { team_id } => {
            let team = client.team(parse_team(&team_id)?);
            let queries = team.queries();
            
            // Query devices on team
            let devices = queries.devices_on_team().await?;
            println!("Team {} diagnostics:", team_id);
            println!("Number of devices on team: {}", devices.iter().count());
            
            // Get current device info for reference
            let current_device_id = client.get_device_id().await?;
            println!("Current device ID: {}", current_device_id);
            
            // Query information for each device
            for device in devices.iter() {
                println!("\nDevice: {}", device);
                
                // Query device role
                match queries.device_role(*device).await {
                    Ok(role) => println!("  Role: {:?}", role),
                    Err(e) => println!("  Role: Error querying role - {}", e),
                }
                
                // // Query device keybundle
                // match queries.device_keybundle(*device).await {
                //     Ok(keybundle) => println!("  Has keybundle: Yes"),
                //     Err(e) => println!("  Has keybundle: Error - {}", e),
                // }
                
                // Query AQC network identifier
                match queries.aqc_net_identifier(*device).await {
                    Ok(Some(net_id)) => println!("  AQC Net ID: {}", net_id.0),
                    Ok(None) => println!("  AQC Net ID: Not assigned"),
                    Err(e) => println!("  AQC Net ID: Error - {}", e),
                }
                
                // Query device label assignments
                match queries.device_label_assignments(*device).await {
                    Ok(labels) => {
                        if labels.iter().count() > 0 {
                            println!("  Assigned labels:");
                            for label in labels.iter() {
                                println!("    {}:{}", label.name, label.id);
                            }
                        } else {
                            println!("  Assigned labels: None");
                        }
                    }
                    Err(e) => println!("  Assigned labels: Error - {}", e),
                }
            }
            
            // Query labels
            match queries.labels().await {
                Ok(labels) => {
                    println!("\nLabels on team: {}", labels.iter().count());
                    for label in labels.iter() {
                        println!("  Label: {}:{}", label.name, label.id);
                    }
                }
                Err(e) => println!("\nLabels: Error querying labels - {}", e),
            }
        }
    }
    Ok(())
}

fn parse_team(s: &str) -> Result<TeamId> { Ok(s.parse()?) }

/// A: Minimal — prints INFO+ by default (no env var required)
fn init_tracing_minimal() {
    tracing_subscriber::fmt().init();
}

/// B: Honor RUST_LOG, but fall back to INFO when RUST_LOG is not set
fn init_tracing_with_env() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).init();
}

async fn generate_and_save_seed(client: &Client, seed_path: &PathBuf) -> Result<[u8; 32]> {
    let mut seed_ikm = [0; 32];
    client.rand(&mut seed_ikm).await;
    
    fs::write(seed_path, &seed_ikm)
        .with_context(|| format!("Failed to save seed to {}", seed_path.display()))?;
    
    Ok(seed_ikm)
}

fn load_seed(seed_path: &PathBuf) -> Result<[u8; 32]> {
    if !seed_path.exists() {
        anyhow::bail!("Seed file does not exist: {}. Create a team first to generate the seed.", seed_path.display());
    }
    
    let seed_data = fs::read(seed_path)
        .with_context(|| format!("Failed to read seed from {}", seed_path.display()))?;
    
    if seed_data.len() != 32 {
        anyhow::bail!("Invalid seed file: expected 32 bytes, got {}", seed_data.len());
    }
    
    let mut seed_ikm = [0; 32];
    seed_ikm.copy_from_slice(&seed_data);
    Ok(seed_ikm)
}
