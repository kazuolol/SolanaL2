//! Solana L2 Validator
//!
//! Main entry point for the L2 gaming chain validator.
//! Supports both leader mode (executes + broadcasts) and validator mode (verifies).

use anyhow::Result;
use clap::{Parser, ValueEnum};
use l2_consensus::{ConsensusConfig, LeaderNode, LeaderNodeBuilder, NodeRole, ValidatorNode, ValidatorNodeBuilder};
use l2_runtime::{AccountStore, BlockProducer, BlockProducerConfig, L2Processor};
use rpc_server::{
    methods::RpcContext, GameHandler, HttpRpcServer, SubscriptionManager, WebSocketServer,
};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod config;

/// Node mode
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    /// Leader mode - executes transactions and broadcasts state
    Leader,
    /// Validator mode - receives state and verifies
    Validator,
}

/// Solana L2 Gaming Chain Validator
#[derive(Parser, Debug)]
#[command(name = "solana-l2")]
#[command(about = "High-performance SVM-based L2 for real-time gaming", long_about = None)]
struct Args {
    /// Node mode (leader or validator)
    #[arg(long, value_enum, default_value = "leader")]
    mode: Mode,

    /// HTTP RPC bind address
    #[arg(long, default_value = "127.0.0.1:8899")]
    rpc_addr: String,

    /// WebSocket bind address
    #[arg(long, default_value = "127.0.0.1:8900")]
    ws_addr: String,

    /// Validator broadcast port (leader mode)
    #[arg(long, default_value = "9000")]
    broadcast_port: u16,

    /// Leader address to connect to (validator mode)
    #[arg(long, default_value = "127.0.0.1:9000")]
    leader_addr: String,

    /// Block time in milliseconds
    #[arg(long, default_value = "33")]
    block_time_ms: u64,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&args.log_level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    match args.mode {
        Mode::Leader => run_leader(args).await,
        Mode::Validator => run_validator(args).await,
    }
}

/// Run in leader mode - execute transactions and broadcast state
async fn run_leader(args: Args) -> Result<()> {
    tracing::info!("Starting Solana L2 Gaming Chain - LEADER MODE");
    tracing::info!("  HTTP RPC: {}", args.rpc_addr);
    tracing::info!("  WebSocket: {}", args.ws_addr);
    tracing::info!("  Broadcast port: {}", args.broadcast_port);
    tracing::info!("  Block time: {}ms ({}Hz)", args.block_time_ms, 1000 / args.block_time_ms);

    // Initialize account store
    let account_store = Arc::new(AccountStore::new());

    // Initialize leader node for broadcasting
    let leader = Arc::new(
        LeaderNodeBuilder::new()
            .broadcast_port(args.broadcast_port)
            .node_id(Pubkey::new_unique())
            .build()
    );

    // Start broadcast server
    leader.start().await?;

    // Initialize L2 processor
    let processor = L2Processor::new(account_store.clone());
    tracing::info!("L2 Processor initialized");

    // Initialize block producer
    let block_config = BlockProducerConfig {
        block_time_ms: args.block_time_ms,
        verbose: args.verbose,
        ..Default::default()
    };
    let block_producer = BlockProducer::new(processor, block_config);

    // Get transaction sender and subscriber
    let tx_sender = block_producer.transaction_sender();
    let mut block_updates = block_producer.subscribe();

    // Initialize subscription manager
    let subscription_manager = Arc::new(SubscriptionManager::new());

    // Set up RPC context
    let current_slot = Arc::new(RwLock::new(0u64));
    let current_blockhash = Arc::new(RwLock::new(solana_sdk::hash::Hash::new_unique()));

    // Create game handler with leader for broadcasting
    let game_handler = GameHandler::with_leader(account_store.clone(), leader.clone());

    let rpc_context = Arc::new(RpcContext {
        account_store: account_store.clone(),
        tx_sender,
        current_slot: current_slot.clone(),
        current_blockhash: current_blockhash.clone(),
        game_handler,
    });

    // Spawn block producer
    let block_producer_handle = tokio::spawn(async move {
        block_producer.run_async().await;
    });

    // Spawn block update handler with leader slot management
    let sub_mgr = subscription_manager.clone();
    let slot_ref = current_slot.clone();
    let hash_ref = current_blockhash.clone();
    let leader_ref = leader.clone();
    let update_handler = tokio::spawn(async move {
        while let Ok(update) = block_updates.recv().await {
            // Begin new slot on leader
            leader_ref.begin_slot(update.slot);

            // Update current slot and blockhash
            *slot_ref.write() = update.slot;
            *hash_ref.write() = update.blockhash;

            // Notify subscribers of account updates
            for (pubkey, account) in &update.modified_accounts {
                sub_mgr.notify_account_update(pubkey, update.slot, account);
            }

            // End slot - broadcasts state changes to validators
            leader_ref.end_slot();

            // Log validator stats periodically
            if update.slot % 100 == 0 {
                let stats = leader_ref.stats();
                tracing::info!(
                    "Slot {}: {} validators connected, {} state changes broadcast",
                    update.slot,
                    stats.connected_validators,
                    stats.state_changes_broadcast
                );
            }
        }
    });

    // Start HTTP RPC server
    let http_context = rpc_context.clone();
    let http_addr = args.rpc_addr.clone();
    let http_server = tokio::spawn(async move {
        let server = HttpRpcServer::new(http_context);
        if let Err(e) = server.run(&http_addr).await {
            tracing::error!("HTTP RPC server error: {}", e);
        }
    });

    // Start WebSocket server
    let ws_context = rpc_context.clone();
    let ws_sub_mgr = subscription_manager.clone();
    let ws_addr = args.ws_addr.clone();
    let ws_server = tokio::spawn(async move {
        let server = WebSocketServer::new(ws_context, ws_sub_mgr);
        if let Err(e) = server.run(&ws_addr).await {
            tracing::error!("WebSocket server error: {}", e);
        }
    });

    tracing::info!("L2 Leader running. Validators can connect to port {}.", args.broadcast_port);
    tracing::info!("Press Ctrl+C to stop.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down...");

    // Abort tasks
    block_producer_handle.abort();
    update_handler.abort();
    http_server.abort();
    ws_server.abort();

    tracing::info!("Leader stopped");

    Ok(())
}

/// Run in validator mode - receive and verify state
async fn run_validator(args: Args) -> Result<()> {
    tracing::info!("Starting Solana L2 Gaming Chain - VALIDATOR MODE");
    tracing::info!("  Connecting to leader: {}", args.leader_addr);

    // Create validator node
    let validator = ValidatorNodeBuilder::new()
        .leader_addr(&args.leader_addr)
        .node_id(Pubkey::new_unique())
        .build();

    // Connect to leader
    validator.connect().await?;

    tracing::info!("Connected to leader. Verifying state changes...");
    tracing::info!("Press Ctrl+C to stop.");

    // Run validator loop (receives and verifies state)
    tokio::select! {
        result = validator.run() => {
            if let Err(e) = result {
                tracing::error!("Validator error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down validator...");
        }
    }

    tracing::info!("Validator stopped");

    Ok(())
}
