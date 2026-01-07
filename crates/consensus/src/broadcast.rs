//! Broadcast server and client for validator network
//!
//! Leader runs BroadcastServer, validators connect with BroadcastClient

use crate::types::{StateChange, ValidatorMessage};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};

/// Broadcast server (run by leader)
pub struct BroadcastServer {
    /// Channel to send state changes to all connected validators
    tx: broadcast::Sender<Vec<u8>>,
    /// Connected validators
    validators: Arc<RwLock<HashMap<Pubkey, ValidatorInfo>>>,
    /// Stats
    stats: Arc<RwLock<ServerStats>>,
}

#[derive(Debug, Clone)]
struct ValidatorInfo {
    pub connected_at: u64,
    pub last_verified_slot: u64,
}

#[derive(Debug, Default)]
struct ServerStats {
    pub messages_broadcast: u64,
    pub validators_connected: usize,
}

impl BroadcastServer {
    /// Create a new broadcast server
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            tx,
            validators: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(ServerStats::default())),
        }
    }

    /// Start listening for validator connections
    pub async fn start(&self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("Broadcast server listening on {}", addr);

        let tx = self.tx.clone();
        let validators = self.validators.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        tracing::info!("Validator connected from {}", peer_addr);
                        let rx = tx.subscribe();
                        let validators = validators.clone();
                        let stats = stats.clone();

                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_validator_connection(stream, rx, validators, stats).await
                            {
                                tracing::warn!("Validator connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("Accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Broadcast a state change to all validators
    pub fn broadcast_state_change(&self, change: &StateChange) {
        let msg = ValidatorMessage::StateChange(change.clone());
        let data = msg.to_bytes();

        match self.tx.send(data) {
            Ok(n) => {
                self.stats.write().messages_broadcast += 1;
                tracing::debug!("Broadcast state change for slot {} to {} validators", change.slot, n);
            }
            Err(_) => {
                // No receivers connected
            }
        }
    }

    /// Get number of connected validators
    pub fn connected_validators(&self) -> usize {
        self.validators.read().len()
    }

    /// Broadcast heartbeat
    pub fn broadcast_heartbeat(&self, slot: u64) {
        let msg = ValidatorMessage::Heartbeat { slot };
        let _ = self.tx.send(msg.to_bytes());
    }
}

async fn handle_validator_connection(
    stream: TcpStream,
    mut rx: broadcast::Receiver<Vec<u8>>,
    validators: Arc<RwLock<HashMap<Pubkey, ValidatorInfo>>>,
    stats: Arc<RwLock<ServerStats>>,
) -> anyhow::Result<()> {
    let ws_stream = accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Track this validator (use a temp ID until they identify)
    let temp_id = Pubkey::new_unique();
    validators.write().insert(
        temp_id,
        ValidatorInfo {
            connected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            last_verified_slot: 0,
        },
    );
    stats.write().validators_connected = validators.read().len();

    // Spawn task to forward broadcasts to this validator
    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(data) => {
                    if ws_sender.send(Message::Binary(data)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Validator lagged {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // Handle incoming messages from validator
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                if let Ok(validator_msg) = ValidatorMessage::from_bytes(&data) {
                    match validator_msg {
                        ValidatorMessage::SlotVerified { slot, validator_id } => {
                            tracing::debug!(
                                "Validator {} verified slot {}",
                                validator_id.to_string()[..8].to_string(),
                                slot
                            );
                            if let Some(info) = validators.write().get_mut(&temp_id) {
                                info.last_verified_slot = slot;
                            }
                        }
                        ValidatorMessage::FraudChallenge { slot, reason, .. } => {
                            tracing::error!(
                                "FRAUD CHALLENGE for slot {}: {}",
                                slot,
                                reason
                            );
                            // In production: halt and investigate
                        }
                        ValidatorMessage::SyncRequest { from_slot } => {
                            tracing::info!("Sync request from slot {}", from_slot);
                            // TODO: Send historical state changes
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::warn!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    send_task.abort();
    validators.write().remove(&temp_id);
    stats.write().validators_connected = validators.read().len();
    tracing::info!("Validator disconnected");

    Ok(())
}

/// Broadcast client (run by validators)
pub struct BroadcastClient {
    /// Channel to receive state changes
    state_rx: mpsc::Receiver<StateChange>,
    /// Channel to send messages to leader
    msg_tx: mpsc::Sender<ValidatorMessage>,
}

impl BroadcastClient {
    /// Connect to leader's broadcast server
    pub async fn connect(leader_addr: &str, node_id: Pubkey) -> anyhow::Result<Self> {
        let url = format!("ws://{}", leader_addr);
        let (ws_stream, _) = connect_async(&url).await?;
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        tracing::info!("Connected to leader at {}", leader_addr);

        let (state_tx, state_rx) = mpsc::channel::<StateChange>(1000);
        let (msg_tx, mut msg_rx) = mpsc::channel::<ValidatorMessage>(100);

        // Spawn receiver task
        tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Binary(data)) => {
                        if let Ok(validator_msg) = ValidatorMessage::from_bytes(&data) {
                            match validator_msg {
                                ValidatorMessage::StateChange(change) => {
                                    let _ = state_tx.send(change).await;
                                }
                                ValidatorMessage::Heartbeat { slot } => {
                                    tracing::trace!("Heartbeat for slot {}", slot);
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        tracing::warn!("Leader closed connection");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Spawn sender task
        tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                let data = msg.to_bytes();
                if ws_sender.send(Message::Binary(data)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self { state_rx, msg_tx })
    }

    /// Receive next state change from leader
    pub async fn recv_state_change(&mut self) -> Option<StateChange> {
        self.state_rx.recv().await
    }

    /// Send slot verified message to leader
    pub async fn send_verified(&self, slot: u64, validator_id: Pubkey) {
        let msg = ValidatorMessage::SlotVerified { slot, validator_id };
        let _ = self.msg_tx.send(msg).await;
    }

    /// Send fraud challenge to leader
    pub async fn send_fraud_challenge(&self, slot: u64, reason: String, evidence: Vec<u8>) {
        let msg = ValidatorMessage::FraudChallenge {
            slot,
            reason,
            evidence,
        };
        let _ = self.msg_tx.send(msg).await;
    }
}
