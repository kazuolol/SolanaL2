//! Core types for consensus and state broadcasting

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// A single account write operation
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct AccountWrite {
    /// Account public key
    pub pubkey: Pubkey,
    /// New account data
    pub data: Vec<u8>,
    /// Account lamports
    pub lamports: u64,
    /// Program owner
    pub owner: Pubkey,
}

/// A batch of state changes for a single slot
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct StateChange {
    /// Slot number this change applies to
    pub slot: u64,
    /// Previous state root hash (for verification chain)
    pub prev_state_root: [u8; 32],
    /// New state root hash after applying changes
    pub new_state_root: [u8; 32],
    /// Account writes in this slot
    pub writes: Vec<AccountWrite>,
    /// Timestamp (unix millis)
    pub timestamp: u64,
    /// Leader's signature over this state change
    pub leader_signature: Vec<u8>,
}

impl StateChange {
    /// Create a new state change
    pub fn new(slot: u64, prev_state_root: [u8; 32]) -> Self {
        Self {
            slot,
            prev_state_root,
            new_state_root: [0u8; 32],
            writes: Vec::new(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            leader_signature: Vec::new(),
        }
    }

    /// Add an account write
    pub fn add_write(&mut self, pubkey: Pubkey, data: Vec<u8>, lamports: u64, owner: Pubkey) {
        self.writes.push(AccountWrite {
            pubkey,
            data,
            lamports,
            owner,
        });
    }

    /// Compute the hash of this state change (for signing)
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.slot.to_le_bytes());
        hasher.update(&self.prev_state_root);
        hasher.update(&self.timestamp.to_le_bytes());

        for write in &self.writes {
            hasher.update(write.pubkey.as_ref());
            hasher.update(&write.data);
            hasher.update(&write.lamports.to_le_bytes());
            hasher.update(write.owner.as_ref());
        }

        *hasher.finalize().as_bytes()
    }

    /// Serialize for network transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("StateChange serialization should not fail")
    }

    /// Deserialize from network
    pub fn from_bytes(data: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(data)
    }
}

/// Message types for validator network
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum ValidatorMessage {
    /// Leader broadcasting a state change
    StateChange(StateChange),

    /// Validator requesting current state (for sync)
    SyncRequest { from_slot: u64 },

    /// Leader responding with state changes for sync
    SyncResponse { changes: Vec<StateChange> },

    /// Validator signaling it has verified a slot
    SlotVerified { slot: u64, validator_id: Pubkey },

    /// Validator challenging a fraudulent state change
    FraudChallenge {
        slot: u64,
        reason: String,
        evidence: Vec<u8>,
    },

    /// Heartbeat to keep connection alive
    Heartbeat { slot: u64 },
}

impl ValidatorMessage {
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("ValidatorMessage serialization should not fail")
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(data)
    }
}

/// Node role in the network
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// Executes transactions and broadcasts state
    Leader,
    /// Receives state, verifies, can challenge
    Validator,
}

/// Configuration for a consensus node
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// This node's role
    pub role: NodeRole,
    /// This node's identity keypair (for signing)
    pub node_id: Pubkey,
    /// Leader's address (for validators to connect)
    pub leader_addr: String,
    /// Port for validator connections (leader only)
    pub broadcast_port: u16,
    /// How many slots between checkpoints
    pub checkpoint_interval: u64,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            role: NodeRole::Leader,
            node_id: Pubkey::default(),
            leader_addr: "127.0.0.1:9000".to_string(),
            broadcast_port: 9000,
            checkpoint_interval: 100,
        }
    }
}

/// Stats about the consensus network
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConsensusStats {
    pub current_slot: u64,
    pub connected_validators: usize,
    pub state_changes_broadcast: u64,
    pub verifications_received: u64,
    pub challenges_received: u64,
    pub last_checkpoint_slot: u64,
}
