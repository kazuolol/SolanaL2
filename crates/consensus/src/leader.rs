//! Leader node - executes transactions and broadcasts state changes

use crate::broadcast::BroadcastServer;
use crate::types::{ConsensusConfig, ConsensusStats, StateChange};
use parking_lot::RwLock;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Leader node that executes and broadcasts state
pub struct LeaderNode {
    /// Broadcast server for validators
    broadcast: Arc<BroadcastServer>,
    /// Current state root
    state_root: RwLock<[u8; 32]>,
    /// Current slot's pending state change
    current_change: RwLock<Option<StateChange>>,
    /// Stats
    stats: RwLock<ConsensusStats>,
    /// Config
    config: ConsensusConfig,
}

impl LeaderNode {
    /// Create a new leader node
    pub fn new(config: ConsensusConfig) -> Self {
        Self {
            broadcast: Arc::new(BroadcastServer::new()),
            state_root: RwLock::new([0u8; 32]),
            current_change: RwLock::new(None),
            stats: RwLock::new(ConsensusStats::default()),
            config,
        }
    }

    /// Start the broadcast server
    pub async fn start(&self) -> anyhow::Result<()> {
        let addr = format!("0.0.0.0:{}", self.config.broadcast_port);
        self.broadcast.start(&addr).await?;
        tracing::info!("Leader node started, broadcasting on port {}", self.config.broadcast_port);
        Ok(())
    }

    /// Begin a new slot - call this at the start of each tick
    pub fn begin_slot(&self, slot: u64) {
        let prev_root = *self.state_root.read();
        let change = StateChange::new(slot, prev_root);
        *self.current_change.write() = Some(change);
        self.stats.write().current_slot = slot;
    }

    /// Record an account write - call this when state changes
    pub fn record_write(&self, pubkey: Pubkey, data: Vec<u8>, lamports: u64, owner: Pubkey) {
        if let Some(ref mut change) = *self.current_change.write() {
            change.add_write(pubkey, data, lamports, owner);
        }
    }

    /// End the slot and broadcast changes - call this at the end of each tick
    pub fn end_slot(&self) {
        let change = self.current_change.write().take();

        if let Some(mut change) = change {
            // Only broadcast if there were writes
            if !change.writes.is_empty() {
                // Compute new state root
                change.new_state_root = change.compute_hash();

                // Update our state root
                *self.state_root.write() = change.new_state_root;

                // Broadcast to validators
                self.broadcast.broadcast_state_change(&change);

                self.stats.write().state_changes_broadcast += 1;

                tracing::debug!(
                    "Slot {} ended: {} writes, {} validators",
                    change.slot,
                    change.writes.len(),
                    self.broadcast.connected_validators()
                );
            }
        }
    }

    /// Get current stats
    pub fn stats(&self) -> ConsensusStats {
        let mut stats = self.stats.read().clone();
        stats.connected_validators = self.broadcast.connected_validators();
        stats
    }

    /// Get connected validator count
    pub fn connected_validators(&self) -> usize {
        self.broadcast.connected_validators()
    }

    /// Send heartbeat (call periodically even without state changes)
    pub fn heartbeat(&self, slot: u64) {
        self.broadcast.broadcast_heartbeat(slot);
    }
}

/// Builder for LeaderNode
pub struct LeaderNodeBuilder {
    config: ConsensusConfig,
}

impl LeaderNodeBuilder {
    pub fn new() -> Self {
        Self {
            config: ConsensusConfig::default(),
        }
    }

    pub fn broadcast_port(mut self, port: u16) -> Self {
        self.config.broadcast_port = port;
        self
    }

    pub fn node_id(mut self, id: Pubkey) -> Self {
        self.config.node_id = id;
        self
    }

    pub fn build(self) -> LeaderNode {
        LeaderNode::new(self.config)
    }
}

impl Default for LeaderNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
