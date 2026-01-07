//! Validator Configuration

use serde::{Deserialize, Serialize};

/// Validator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// HTTP RPC bind address
    pub rpc_addr: String,
    /// WebSocket bind address
    pub ws_addr: String,
    /// Block time in milliseconds
    pub block_time_ms: u64,
    /// L1 RPC URL (for future integration)
    pub l1_rpc_url: Option<String>,
    /// Delegation program ID (for future integration)
    pub delegation_program_id: Option<String>,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            rpc_addr: "127.0.0.1:8899".to_string(),
            ws_addr: "127.0.0.1:8900".to_string(),
            block_time_ms: 33,
            l1_rpc_url: None,
            delegation_program_id: None,
        }
    }
}
