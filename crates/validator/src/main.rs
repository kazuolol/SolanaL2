//! Solana L2 Validator
//!
//! Main entry point for the L2 gaming chain validator.
//! Supports both leader mode (executes + broadcasts) and validator mode (verifies).
//! State is persisted to disk and survives restarts.

use anyhow::Result;
use clap::{Parser, ValueEnum};
use l2_consensus::{LeaderNodeBuilder, ValidatorNodeBuilder};
use l2_runtime::{
    AccountStore, AccountStorePersistence, BlockProducer, BlockProducerConfig,
    ChainMetadata, L2Processor, PersistentStore,
};
use rpc_server::{
    methods::RpcContext, HttpRpcServer, SubscriptionManager, WebSocketServer,
};
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod config;

use solana_sdk::account::{Account, AccountSharedData};

/// Create the default world account if it doesn't exist
fn create_default_world(account_store: &AccountStore, slot: u64) {
    // World program ID
    let world_program_id = world_program::id();

    // Default world name
    let mut name_bytes = [0u8; 32];
    name_bytes[..7].copy_from_slice(b"default");

    // Derive world PDA
    let (world_pda, bump) = Pubkey::find_program_address(
        &[b"world", &name_bytes],
        &world_program_id,
    );

    // Check if world already exists
    if account_store.get_account(&world_pda).is_some() {
        tracing::info!("Default world already exists: {}", world_pda);
        return;
    }

    // Create world config using Borsh serialization
    // WorldConfig layout: name[32] + authority[32] + width[4] + depth[4] + max_players[2] + player_count[2] + tick_rate[1] + bump[1] + l1_game[32] + init_ts[8]
    let mut data = Vec::with_capacity(118);
    data.extend_from_slice(&name_bytes); // name: [u8; 32]
    data.extend_from_slice(&[0u8; 32]); // authority: Pubkey (default)
    data.extend_from_slice(&100u32.to_le_bytes()); // width: u32
    data.extend_from_slice(&100u32.to_le_bytes()); // depth: u32
    data.extend_from_slice(&100u16.to_le_bytes()); // max_players: u16
    data.extend_from_slice(&0u16.to_le_bytes()); // player_count: u16
    data.push(30); // tick_rate: u8
    data.push(bump); // bump: u8
    data.extend_from_slice(&[0u8; 32]); // l1_game: Pubkey (default)
    let init_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    data.extend_from_slice(&init_ts.to_le_bytes()); // init_ts: i64

    let account = AccountSharedData::from(Account {
        lamports: 1,
        data,
        owner: world_program_id,
        executable: false,
        rent_epoch: 0,
    });

    account_store.store_account(world_pda, account, slot);
    tracing::info!("Created default world: {}", world_pda);
}

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

    /// Data directory for persistent state
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// Save state every N slots (0 = only on shutdown)
    #[arg(long, default_value = "300")]
    save_interval: u64,
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
    tracing::info!("  Data directory: {:?}", args.data_dir);
    tracing::info!("  Save interval: {} slots", args.save_interval);

    // Create data directory if it doesn't exist
    std::fs::create_dir_all(&args.data_dir)?;

    // Open persistent store
    let persistent_store = Arc::new(PersistentStore::open(&args.data_dir)?);

    // Initialize account store
    let account_store = Arc::new(AccountStore::new());

    // Load existing state from disk
    let loaded_metadata = persistent_store.load_metadata()?;
    let start_slot = if let Some(ref metadata) = loaded_metadata {
        tracing::info!(
            "Loading state from disk: slot {}, {} accounts",
            metadata.slot,
            metadata.account_count
        );
        let loaded = account_store.load_from_disk(&persistent_store)?;
        tracing::info!("Loaded {} accounts from persistent storage", loaded);
        metadata.slot
    } else {
        tracing::info!("No existing state found, starting fresh");
        0
    };

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

    // Create default world account if it doesn't exist
    create_default_world(&account_store, 0);

    let rpc_context = Arc::new(RpcContext {
        account_store: account_store.clone(),
        tx_sender,
        current_slot: current_slot.clone(),
        current_blockhash: current_blockhash.clone(),
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
    let persist_store = persistent_store.clone();
    let persist_accounts = account_store.clone();
    let save_interval = args.save_interval;
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

            // Periodic save to disk
            if save_interval > 0 && update.slot % save_interval == 0 && update.slot > 0 {
                let metadata = ChainMetadata {
                    slot: update.slot,
                    blockhash: update.blockhash.to_bytes(),
                    epoch: update.slot / 432000, // ~2 days at 30Hz
                    account_count: persist_accounts.len() as u64,
                    last_save_ts: chrono::Utc::now().timestamp(),
                };
                if let Err(e) = persist_store.save_metadata(&metadata) {
                    tracing::error!("Failed to save metadata: {}", e);
                }
                if let Err(e) = persist_accounts.save_to_disk(&persist_store) {
                    tracing::error!("Failed to save accounts: {}", e);
                } else {
                    tracing::info!("Saved state at slot {}", update.slot);
                }
            }

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

    // Save state before shutdown
    let final_slot = *current_slot.read();
    let final_hash = *current_blockhash.read();
    tracing::info!("Saving final state at slot {}...", final_slot);

    let metadata = ChainMetadata {
        slot: final_slot,
        blockhash: final_hash.to_bytes(),
        epoch: final_slot / 432000,
        account_count: account_store.len() as u64,
        last_save_ts: chrono::Utc::now().timestamp(),
    };

    if let Err(e) = persistent_store.save_metadata(&metadata) {
        tracing::error!("Failed to save final metadata: {}", e);
    }
    if let Err(e) = account_store.save_to_disk(&persistent_store) {
        tracing::error!("Failed to save final state: {}", e);
    } else {
        tracing::info!("Final state saved: {} accounts at slot {}", account_store.len(), final_slot);
    }

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
