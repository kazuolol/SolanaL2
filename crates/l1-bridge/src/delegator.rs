//! Account Delegator - Fetches delegated accounts from L1
//!
//! Currently a stub implementation since L1 programs aren't deployed.
//! When L1 is ready, this will use solana-client to fetch delegated accounts.

use l1_integration::L1User;
use solana_sdk::pubkey::Pubkey;

/// Fetches delegated accounts from Solana L1 (stub implementation)
pub struct AccountDelegator {
    /// L1 RPC URL (stored for future use)
    rpc_url: String,
    /// L1 program ID (for future use)
    l1_program_id: Option<Pubkey>,
}

impl AccountDelegator {
    /// Create a new account delegator
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            l1_program_id: None,
        }
    }

    /// Create with a specific L1 program ID
    pub fn with_program_id(rpc_url: &str, program_id: Pubkey) -> Self {
        let mut delegator = Self::new(rpc_url);
        delegator.l1_program_id = Some(program_id);
        delegator
    }

    /// Get the RPC URL
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Fetch and deserialize an L1 User account (stub - returns None)
    ///
    /// When L1 is deployed, this will:
    /// 1. Connect to L1 RPC
    /// 2. Fetch account data
    /// 3. Deserialize to L1User
    pub async fn fetch_l1_user(&self, _pubkey: &Pubkey) -> anyhow::Result<Option<L1User>> {
        // L1 not deployed, return None
        tracing::debug!("fetch_l1_user: L1 not deployed, returning None");
        Ok(None)
    }

    /// Check if an account is delegated to our L2 (stub - returns false)
    ///
    /// When delegation program is deployed, this will check DelegationRecord on L1.
    pub async fn is_delegated(&self, _pubkey: &Pubkey) -> anyhow::Result<bool> {
        // Delegation program not deployed, always return false
        Ok(false)
    }
}
