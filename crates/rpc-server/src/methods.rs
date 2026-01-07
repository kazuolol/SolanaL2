//! RPC Methods - JSON-RPC method handlers
//!
//! Implements Solana-compatible RPC methods for the L2.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crate::game_handler::GameHandler;
use l2_runtime::{AccountStore, TransactionSender};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
    signature::Signature,
    transaction::VersionedTransaction,
};
use std::{str::FromStr, sync::Arc};
use parking_lot::RwLock;

/// RPC context shared across handlers
pub struct RpcContext {
    pub account_store: Arc<AccountStore>,
    pub tx_sender: TransactionSender,
    pub current_slot: Arc<RwLock<Slot>>,
    pub current_blockhash: Arc<RwLock<Hash>>,
    pub game_handler: GameHandler,
}

// ============ Request/Response Types ============

#[derive(Debug, Serialize, Deserialize)]
pub struct SendTransactionRequest {
    pub transaction: String, // Base64 encoded
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetAccountInfoRequest {
    pub pubkey: String,
    #[serde(default)]
    pub encoding: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcContext_ {
    pub slot: Slot,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountInfo {
    pub data: (String, String), // (data, encoding)
    pub executable: bool,
    pub lamports: u64,
    pub owner: String,
    #[serde(rename = "rentEpoch")]
    pub rent_epoch: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetAccountInfoResponse {
    pub context: RpcContext_,
    pub value: Option<AccountInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockhashInfo {
    pub blockhash: String,
    #[serde(rename = "lastValidBlockHeight")]
    pub last_valid_block_height: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetLatestBlockhashResponse {
    pub context: RpcContext_,
    pub value: BlockhashInfo,
}

// ============ RPC Handlers ============

/// Handle sendTransaction RPC method
pub fn handle_send_transaction(
    ctx: &RpcContext,
    params: SendTransactionRequest,
) -> Result<String, RpcError> {
    // Decode transaction
    let tx_bytes = BASE64
        .decode(&params.transaction)
        .map_err(|_| RpcError::InvalidParams("Invalid base64 encoding".to_string()))?;

    let tx: VersionedTransaction = bincode::deserialize(&tx_bytes)
        .map_err(|_| RpcError::InvalidParams("Invalid transaction format".to_string()))?;

    // Get signature before consuming
    let signature = tx.signatures[0];

    // Convert to sanitized transaction
    let sanitized = solana_sdk::transaction::SanitizedTransaction::try_create(
        tx,
        solana_sdk::transaction::MessageHash::Compute,
        None,
        solana_sdk::transaction::SimpleAddressLoader::Disabled,
        &solana_sdk::reserved_account_keys::ReservedAccountKeys::empty_key_set(),
    )
    .map_err(|e| RpcError::InvalidParams(format!("Cannot sanitize transaction: {:?}", e)))?;

    // Submit to block producer
    ctx.tx_sender
        .send(sanitized)
        .map_err(|e| RpcError::InternalError(e))?;

    Ok(signature.to_string())
}

/// Handle getAccountInfo RPC method
pub fn handle_get_account_info(
    ctx: &RpcContext,
    params: GetAccountInfoRequest,
) -> Result<GetAccountInfoResponse, RpcError> {
    let pubkey = Pubkey::from_str(&params.pubkey)
        .map_err(|_| RpcError::InvalidParams("Invalid pubkey".to_string()))?;

    let slot = *ctx.current_slot.read();

    let value = ctx.account_store.get_account(&pubkey).map(|account| {
        use solana_sdk::account::ReadableAccount;

        let encoding = params.encoding.as_deref().unwrap_or("base64");
        let data = match encoding {
            "base58" => (bs58::encode(account.data()).into_string(), "base58".to_string()),
            _ => (BASE64.encode(account.data()), "base64".to_string()),
        };

        AccountInfo {
            data,
            executable: account.executable(),
            lamports: account.lamports(),
            owner: account.owner().to_string(),
            rent_epoch: 0,
        }
    });

    Ok(GetAccountInfoResponse {
        context: RpcContext_ { slot },
        value,
    })
}

/// Handle getLatestBlockhash RPC method
pub fn handle_get_latest_blockhash(ctx: &RpcContext) -> Result<GetLatestBlockhashResponse, RpcError> {
    let slot = *ctx.current_slot.read();
    let blockhash = *ctx.current_blockhash.read();

    Ok(GetLatestBlockhashResponse {
        context: RpcContext_ { slot },
        value: BlockhashInfo {
            blockhash: blockhash.to_string(),
            last_valid_block_height: slot + 150, // Valid for ~5 seconds at 30Hz
        },
    })
}

/// Handle getSlot RPC method
pub fn handle_get_slot(ctx: &RpcContext) -> Result<Slot, RpcError> {
    Ok(*ctx.current_slot.read())
}

/// Handle getHealth RPC method
pub fn handle_get_health() -> Result<String, RpcError> {
    Ok("ok".to_string())
}

// ============ Error Types ============

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Invalid params: {0}")]
    InvalidParams(String),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Method not found: {0}")]
    MethodNotFound(String),
}
