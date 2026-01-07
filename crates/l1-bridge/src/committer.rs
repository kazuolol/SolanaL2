//! State Committer - Commits L2 state back to L1
//!
//! Currently a stub implementation since delegation program isn't deployed.
//! When ready, this will build and send transactions to L1.

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Commits L2 state changes back to Solana L1 (stub implementation)
pub struct StateCommitter {
    /// Validator keypair for signing commits
    validator_keypair: Arc<Keypair>,
    /// L1 RPC URL (stored for future use)
    rpc_url: String,
    /// Delegation program ID (not deployed yet)
    delegation_program_id: Option<Pubkey>,
    /// Commit interval in L2 slots
    commit_interval_slots: u64,
    /// Last committed L2 slot
    last_commit_slot: RwLock<u64>,
}

impl StateCommitter {
    /// Create a new state committer
    pub fn new(rpc_url: &str, validator_keypair: Keypair) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            validator_keypair: Arc::new(validator_keypair),
            delegation_program_id: None,
            commit_interval_slots: 100, // ~3.3 seconds at 30Hz
            last_commit_slot: RwLock::new(0),
        }
    }

    /// Set the delegation program ID
    pub fn with_delegation_program(mut self, program_id: Pubkey) -> Self {
        self.delegation_program_id = Some(program_id);
        self
    }

    /// Set the commit interval
    pub fn with_commit_interval(mut self, slots: u64) -> Self {
        self.commit_interval_slots = slots;
        self
    }

    /// Get the RPC URL
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Check if we should commit based on current slot
    pub async fn should_commit(&self, current_slot: u64) -> bool {
        let last_slot = *self.last_commit_slot.read().await;
        current_slot - last_slot >= self.commit_interval_slots
    }

    /// Commit state to L1 (stub - delegation program not deployed)
    ///
    /// When delegation program is deployed, this will:
    /// 1. Build a transaction calling delegation_program::commit_state
    /// 2. Sign with validator keypair
    /// 3. Send to L1 RPC
    pub async fn commit_state(
        &self,
        _account_pubkey: &Pubkey,
        _new_data: Vec<u8>,
        l2_slot: u64,
    ) -> anyhow::Result<Option<Signature>> {
        tracing::debug!(
            "Would commit state at L2 slot {} (delegation program not deployed)",
            l2_slot
        );

        *self.last_commit_slot.write().await = l2_slot;

        // Return None since we're not actually committing
        Ok(None)
    }

    /// Get validator public key
    pub fn validator_pubkey(&self) -> Pubkey {
        self.validator_keypair.pubkey()
    }

    /// Get last commit slot
    pub async fn last_commit_slot(&self) -> u64 {
        *self.last_commit_slot.read().await
    }
}
