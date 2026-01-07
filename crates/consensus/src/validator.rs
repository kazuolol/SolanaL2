//! Validator node - receives state changes, verifies, and can challenge fraud

use crate::broadcast::BroadcastClient;
use crate::types::{ConsensusConfig, NodeRole, StateChange};
use parking_lot::RwLock;
use solana_sdk::{
    account::AccountSharedData,
    pubkey::Pubkey,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Validator node that verifies leader's state changes
pub struct ValidatorNode {
    /// Client connected to leader
    client: RwLock<Option<BroadcastClient>>,
    /// Local copy of account state (for verification)
    accounts: RwLock<HashMap<Pubkey, AccountSharedData>>,
    /// Current state root (should match leader)
    state_root: RwLock<[u8; 32]>,
    /// Last verified slot
    last_verified_slot: RwLock<u64>,
    /// Config
    config: ConsensusConfig,
    /// This node's ID
    node_id: Pubkey,
}

impl ValidatorNode {
    /// Create a new validator node
    pub fn new(config: ConsensusConfig) -> Self {
        let node_id = config.node_id;
        Self {
            client: RwLock::new(None),
            accounts: RwLock::new(HashMap::new()),
            state_root: RwLock::new([0u8; 32]),
            last_verified_slot: RwLock::new(0),
            config,
            node_id,
        }
    }

    /// Connect to the leader
    pub async fn connect(&self) -> anyhow::Result<()> {
        let client = BroadcastClient::connect(&self.config.leader_addr, self.node_id).await?;
        *self.client.write() = Some(client);
        tracing::info!("Validator connected to leader at {}", self.config.leader_addr);
        Ok(())
    }

    /// Run the validator loop - receives and verifies state changes
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut client = self.client.write().take()
            .ok_or_else(|| anyhow::anyhow!("Not connected to leader"))?;

        tracing::info!("Validator running, waiting for state changes...");

        loop {
            match client.recv_state_change().await {
                Some(change) => {
                    match self.verify_and_apply(&change) {
                        Ok(()) => {
                            // Send verification to leader
                            client.send_verified(change.slot, self.node_id).await;
                            *self.last_verified_slot.write() = change.slot;

                            tracing::info!(
                                "Verified slot {}: {} writes",
                                change.slot,
                                change.writes.len()
                            );
                        }
                        Err(e) => {
                            // Fraud detected!
                            tracing::error!("FRAUD DETECTED at slot {}: {}", change.slot, e);
                            client.send_fraud_challenge(
                                change.slot,
                                e.to_string(),
                                Vec::new(), // TODO: Include evidence
                            ).await;
                        }
                    }
                }
                None => {
                    tracing::warn!("Leader connection lost");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Verify a state change and apply it locally
    fn verify_and_apply(&self, change: &StateChange) -> anyhow::Result<()> {
        // Verify the state change is valid

        // 1. Check slot is sequential
        let last_slot = *self.last_verified_slot.read();
        if change.slot != 0 && change.slot != last_slot + 1 {
            // Allow gaps for now, but log
            tracing::warn!(
                "Slot gap: expected {} or {}, got {}",
                last_slot,
                last_slot + 1,
                change.slot
            );
        }

        // 2. Verify prev_state_root matches our state
        let our_root = *self.state_root.read();
        if our_root != [0u8; 32] && change.prev_state_root != our_root {
            return Err(anyhow::anyhow!(
                "State root mismatch: expected {:?}, got {:?}",
                our_root,
                change.prev_state_root
            ));
        }

        // 3. Verify the hash computation
        let computed_hash = change.compute_hash();
        if computed_hash != change.new_state_root {
            return Err(anyhow::anyhow!(
                "State root hash mismatch: computed {:?}, claimed {:?}",
                computed_hash,
                change.new_state_root
            ));
        }

        // 4. Apply the changes locally
        let mut accounts = self.accounts.write();
        for write in &change.writes {
            let account = AccountSharedData::from(solana_sdk::account::Account {
                lamports: write.lamports,
                data: write.data.clone(),
                owner: write.owner,
                executable: false,
                rent_epoch: 0,
            });
            accounts.insert(write.pubkey, account);
        }

        // 5. Update our state root
        *self.state_root.write() = change.new_state_root;

        Ok(())
    }

    /// Get an account from local state
    pub fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.accounts.read().get(pubkey).cloned()
    }

    /// Get last verified slot
    pub fn last_verified_slot(&self) -> u64 {
        *self.last_verified_slot.read()
    }

    /// Get current state root
    pub fn state_root(&self) -> [u8; 32] {
        *self.state_root.read()
    }
}

/// Builder for ValidatorNode
pub struct ValidatorNodeBuilder {
    config: ConsensusConfig,
}

impl ValidatorNodeBuilder {
    pub fn new() -> Self {
        Self {
            config: ConsensusConfig {
                role: NodeRole::Validator,
                ..Default::default()
            },
        }
    }

    pub fn leader_addr(mut self, addr: &str) -> Self {
        self.config.leader_addr = addr.to_string();
        self
    }

    pub fn node_id(mut self, id: Pubkey) -> Self {
        self.config.node_id = id;
        self
    }

    pub fn build(self) -> ValidatorNode {
        ValidatorNode::new(self.config)
    }
}

impl Default for ValidatorNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
