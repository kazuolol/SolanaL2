//! RPC Server - JSON-RPC and WebSocket for L2
//!
//! Provides Solana-compatible RPC interface:
//! - HTTP JSON-RPC: sendTransaction, getAccountInfo, getLatestBlockhash, etc.
//! - WebSocket: accountSubscribe, accountUnsubscribe

pub mod http_server;
pub mod methods;
pub mod subscriptions;
pub mod ws_server;

pub use http_server::HttpRpcServer;
pub use subscriptions::SubscriptionManager;
pub use ws_server::WebSocketServer;

// Re-export types that consumers might need
pub use l2_runtime::{BlockUpdate, TransactionSender};

/// RPC Server configuration
#[derive(Clone, Debug)]
pub struct RpcServerConfig {
    /// HTTP RPC bind address
    pub http_addr: String,
    /// WebSocket bind address
    pub ws_addr: String,
    /// Enable verbose logging
    pub verbose: bool,
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            http_addr: "127.0.0.1:8899".to_string(),
            ws_addr: "127.0.0.1:8900".to_string(),
            verbose: false,
        }
    }
}

/// Combined RPC server (HTTP + WebSocket)
pub struct RpcServer {
    config: RpcServerConfig,
}

impl RpcServer {
    pub fn new(config: RpcServerConfig) -> Self {
        Self { config }
    }

    /// Get the HTTP address
    pub fn http_addr(&self) -> &str {
        &self.config.http_addr
    }

    /// Get the WebSocket address
    pub fn ws_addr(&self) -> &str {
        &self.config.ws_addr
    }
}
