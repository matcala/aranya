//! UDP traffic forwarding through AQC channels.

use std::{net::SocketAddr, sync::Arc};
use anyhow::Result;
use bytes::Bytes;
use tokio::{net::UdpSocket, task::JoinSet};
use tracing::{error, info, warn};

use aranya_client::aqc::AqcBidiChannel;
use aranya_util::Addr;

/// UDP forwarder that bridges UDP traffic through AQC channels.
#[derive(Debug)]
pub struct UdpForwarder {
    listen_socket: Arc<UdpSocket>,
    target_addr: SocketAddr,
}

impl UdpForwarder {
    /// Create a new UDP forwarder that listens on `listen_addr` and forwards to `target_addr`.
    pub async fn new(listen_addr: Addr, target_addr: Addr) -> Result<Self> {
        let listen_socket_addr: SocketAddr = SocketAddr::from(([127,0,0,1], listen_addr.port()));
        let target_socket_addr: SocketAddr = SocketAddr::from(([127,0,0,1], target_addr.port()));
        
        let listen_socket = Arc::new(UdpSocket::bind(listen_socket_addr).await?);
        info!("UDP forwarder listening on {} -> forwarding to {}", listen_socket_addr, target_socket_addr);
        
        Ok(Self {
            listen_socket,
            target_addr: target_socket_addr,
        })
    }

    /// Start forwarding UDP traffic through the AQC channel (as sender).
    /// Listens for UDP packets (COSMOS commands) and forwards them through AQC;
    /// Forwards AQC responses (telemtry) back to COSMOS.
    pub async fn start_forwarding_as_sender(&self, mut aqc_channel: AqcBidiChannel) -> Result<()> {
        let mut join_set = JoinSet::new();
        
        // Create persistent unidirectional send stream for requests
        info!("Creating persistent AQC send stream for requests");
        let mut send_stream = aqc_channel.create_uni_stream().await?;
        info!("Created persistent AQC send stream for requests");
        
        // Handle incoming UDP packets and forward them through the persistent AQC send stream
        let listen_socket = self.listen_socket.clone();
        
        join_set.spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                match listen_socket.recv(&mut buf).await {
                    Ok(len) => {
                        let data = Bytes::copy_from_slice(&buf[..len]);
                        info!("Received {} bytes from UDP client, forwarding through AQC", len);
                        
                        // Send the data through persistent AQC stream (no need to include address)
                        if let Err(e) = send_stream.send(data).await {
                            error!("Failed to send data through AQC send stream: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to receive UDP packet: {}", e);
                        break;
                    }
                }
            }
        });

        // Handle incoming AQC receive stream for responses and forward to target
        let target_socket = UdpSocket::bind("127.0.0.1:0").await?;
        let target_addr = self.target_addr;
        
        join_set.spawn(async move {
            info!("Waiting for AQC receive stream for responses");
            match aqc_channel.receive_stream().await {
                Ok(aranya_client::aqc::AqcPeerStream::Receive(mut recv_stream)) => {
                    info!("Received AQC receive stream, starting to forward responses to {}", target_addr);
                    loop {
                        match recv_stream.receive().await {
                            Ok(Some(data)) => {
                                info!("Received {} bytes from AQC, forwarding to target {}", data.len(), target_addr);
                                
                                // Forward response to target address
                                if let Err(e) = target_socket.send_to(&data, target_addr).await {
                                    error!("Failed to send UDP response to {}: {}", target_addr, e);
                                }
                            }
                            Ok(None) => {
                                info!("AQC receive stream closed");
                                break;
                            }
                            Err(e) => {
                                error!("Failed to receive from AQC stream: {}", e);
                                break;
                            }
                        }
                    }
                }
                Ok(_) => {
                    warn!("Received non-receive stream, expected receive stream");
                }
                Err(e) => {
                    error!("Failed to receive AQC stream: {}", e);
                }
            }
        });

        // Wait for all tasks to complete
        join_set.join_all().await;
        
        Ok(())
    }

    /// Start forwarding UDP traffic through the AQC channel (as receiver).
    /// Receives AQC data (COSMOS commands), forwards to target;
    /// Receives telemetry (from target) and pipes through AQC.
    pub async fn start_forwarding_as_receiver(&self, mut aqc_channel: AqcBidiChannel) -> Result<()> {
        let mut join_set = JoinSet::new();
        
        // Create persistent unidirectional send stream for responses
        info!("Creating persistent AQC send stream for responses");
        let mut send_stream = aqc_channel.create_uni_stream().await?;
        info!("Created persistent AQC send stream for responses");
        
        // Handle incoming AQC receive stream and forward to target
        let target_socket = UdpSocket::bind("127.0.0.1:0").await?;
        let target_addr = self.target_addr;
        
        join_set.spawn(async move {
            info!("Waiting for AQC receive stream for requests");
            match aqc_channel.receive_stream().await {
                Ok(aranya_client::aqc::AqcPeerStream::Receive(mut recv_stream)) => {
                    info!("Received AQC receive stream, starting to forward requests to {}", target_addr);
                    loop {
                        match recv_stream.receive().await {
                            Ok(Some(data)) => {
                                info!("Received {} bytes from AQC, forwarding to target {}", data.len(), target_addr);
                                
                                // Forward request to target address
                                if let Err(e) = target_socket.send_to(&data, target_addr).await {
                                    error!("Failed to send UDP request to {}: {}", target_addr, e);
                                }
                            }
                            Ok(None) => {
                                info!("AQC receive stream closed");
                                break;
                            }
                            Err(e) => {
                                error!("Failed to receive from AQC stream: {}", e);
                                break;
                            }
                        }
                    }
                }
                Ok(_) => {
                    warn!("Received non-receive stream, expected receive stream");
                }
                Err(e) => {
                    error!("Failed to receive AQC stream: {}", e);
                }
            }
        });

        // Handle incoming UDP responses from target and forward through AQC
        let listen_socket = self.listen_socket.clone();
        
        join_set.spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                match listen_socket.recv(&mut buf).await {
                    Ok(len) => {
                        let data = Bytes::copy_from_slice(&buf[..len]);
                        info!("Received {} bytes from target, forwarding through AQC", len);
                        
                        // Send response through AQC
                        if let Err(e) = send_stream.send(data).await {
                            error!("Failed to send response through AQC: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to receive UDP response from target: {}", e);
                        break;
                    }
                }
            }
        });

        // Wait for all tasks to complete
        join_set.join_all().await;
        
        Ok(())
    }
}