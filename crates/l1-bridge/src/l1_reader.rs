//! L1 Reader - Reads state from Solana L1
//!
//! For future integration with L1 economic layer.
//! Currently placeholder since L1 programs aren't deployed.

use l1_integration::{L1User, WeaponStats};
use solana_sdk::pubkey::Pubkey;

/// Reads L1 state (placeholder for future integration)
pub struct L1Reader {
    /// L1 RPC URL
    _rpc_url: String,
}

impl L1Reader {
    /// Create a new L1 reader
    pub fn new(rpc_url: &str) -> Self {
        Self {
            _rpc_url: rpc_url.to_string(),
        }
    }

    /// Check if user is in PVP zone (L1 in_game.state)
    ///
    /// Note: This does NOT gate L2 world entry.
    /// It's for future sync with L1 item risk mechanics.
    pub async fn is_in_pvp_zone(&self, _user_pda: &Pubkey) -> anyhow::Result<bool> {
        // L1 not deployed, return false
        Ok(false)
    }

    /// Get weapon stats from L1 inventory (placeholder)
    ///
    /// In production, this would:
    /// 1. Fetch L1 User account
    /// 2. Find equipped weapon token
    /// 3. Fetch weapon NFT metadata for stats
    pub async fn get_weapon_stats(&self, _user_pda: &Pubkey) -> anyhow::Result<Option<WeaponStats>> {
        // L1 not deployed, return None (will use defaults)
        Ok(None)
    }

    /// Get user inventory from L1 (placeholder)
    pub async fn get_user_inventory(&self, _user_pda: &Pubkey) -> anyhow::Result<Option<L1User>> {
        // L1 not deployed, return None
        Ok(None)
    }
}
