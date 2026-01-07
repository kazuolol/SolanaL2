//! Block Producer - 30Hz game loop
//!
//! Produces blocks at 30Hz (~33ms intervals) for real-time game state updates.
//! This is comparable to Fortnite's server tick rate.

use crate::{processor::L2Processor, TransactionResult, BLOCK_TIME_MS, MAX_TXS_PER_BLOCK};
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use solana_sdk::{
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
    account::AccountSharedData,
    transaction::SanitizedTransaction,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

/// Block update event sent to subscribers
#[derive(Clone, Debug)]
pub struct BlockUpdate {
    /// Slot (block height)
    pub slot: Slot,
    /// Blockhash for this block
    pub blockhash: Hash,
    /// Number of transactions processed
    pub transaction_count: usize,
    /// Accounts modified in this block
    pub modified_accounts: Vec<(Pubkey, AccountSharedData)>,
    /// Transaction results
    pub transaction_results: Vec<TransactionResult>,
    /// Block production time in microseconds
    pub processing_time_us: u64,
}

/// Block producer configuration
#[derive(Clone, Debug)]
pub struct BlockProducerConfig {
    /// Block time in milliseconds (default: 33ms for 30Hz)
    pub block_time_ms: u64,
    /// Maximum transactions per block
    pub max_txs_per_block: usize,
    /// Enable verbose logging
    pub verbose: bool,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            block_time_ms: BLOCK_TIME_MS,
            max_txs_per_block: MAX_TXS_PER_BLOCK,
            verbose: false,
        }
    }
}

/// Handle for submitting transactions to the block producer
#[derive(Clone)]
pub struct TransactionSender {
    sender: Sender<SanitizedTransaction>,
}

impl TransactionSender {
    /// Submit a transaction for processing
    pub fn send(&self, tx: SanitizedTransaction) -> Result<(), String> {
        self.sender
            .try_send(tx)
            .map_err(|e| format!("Failed to submit transaction: {}", e))
    }
}

/// Block Producer
///
/// Runs the 30Hz game loop, processing transactions and producing blocks.
pub struct BlockProducer {
    /// Transaction processor
    processor: L2Processor,
    /// Transaction receiver
    tx_receiver: Receiver<SanitizedTransaction>,
    /// Transaction sender (for cloning)
    tx_sender: Sender<SanitizedTransaction>,
    /// Block update broadcaster
    update_sender: broadcast::Sender<BlockUpdate>,
    /// Configuration
    config: BlockProducerConfig,
    /// Running flag
    running: Arc<AtomicBool>,
}

impl BlockProducer {
    /// Create a new block producer
    pub fn new(processor: L2Processor, config: BlockProducerConfig) -> Self {
        // Create transaction channel with bounded capacity
        let (tx_sender, tx_receiver) = bounded(1024);

        // Create broadcast channel for block updates
        let (update_sender, _) = broadcast::channel(64);

        Self {
            processor,
            tx_receiver,
            tx_sender,
            update_sender,
            config,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a sender for submitting transactions
    pub fn transaction_sender(&self) -> TransactionSender {
        TransactionSender {
            sender: self.tx_sender.clone(),
        }
    }

    /// Subscribe to block updates
    pub fn subscribe(&self) -> broadcast::Receiver<BlockUpdate> {
        self.update_sender.subscribe()
    }

    /// Check if the block producer is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stop the block producer
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get current slot
    pub fn current_slot(&self) -> Slot {
        self.processor.current_slot()
    }

    /// Get current blockhash
    pub fn current_blockhash(&self) -> Hash {
        self.processor.current_blockhash()
    }

    /// Run the block producer (blocking)
    ///
    /// This should be spawned on a dedicated thread.
    pub fn run(&mut self) {
        self.running.store(true, Ordering::SeqCst);

        let block_duration = Duration::from_millis(self.config.block_time_ms);
        let mut pending_txs: Vec<SanitizedTransaction> = Vec::with_capacity(self.config.max_txs_per_block);
        let mut last_log_slot = 0;

        tracing::info!(
            "Block producer started ({}ms blocks, {}Hz)",
            self.config.block_time_ms,
            1000 / self.config.block_time_ms
        );

        while self.running.load(Ordering::SeqCst) {
            let tick_start = Instant::now();

            // Drain transaction queue
            loop {
                match self.tx_receiver.try_recv() {
                    Ok(tx) => {
                        pending_txs.push(tx);
                        if pending_txs.len() >= self.config.max_txs_per_block {
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        tracing::warn!("Transaction channel disconnected");
                        self.running.store(false, Ordering::SeqCst);
                        return;
                    }
                }
            }

            // Process transactions
            let mut transaction_results = Vec::new();
            let mut modified_accounts = Vec::new();

            if !pending_txs.is_empty() {
                let results = self.processor.process_transactions(&pending_txs);

                for result in results {
                    if result.success {
                        modified_accounts.extend(result.modified_accounts.clone());
                    }
                    transaction_results.push(result);
                }
            }

            let tx_count = pending_txs.len();
            pending_txs.clear();

            // Advance slot
            self.processor.advance_slot();

            let processing_time = tick_start.elapsed();

            // Create and broadcast block update
            let update = BlockUpdate {
                slot: self.processor.current_slot(),
                blockhash: self.processor.current_blockhash(),
                transaction_count: tx_count,
                modified_accounts,
                transaction_results,
                processing_time_us: processing_time.as_micros() as u64,
            };

            // Broadcast to subscribers (ignore errors if no subscribers)
            let _ = self.update_sender.send(update);

            // Log periodically
            if self.config.verbose || (self.processor.current_slot() - last_log_slot >= 300) {
                // Every ~10 seconds
                if tx_count > 0 || self.config.verbose {
                    tracing::debug!(
                        "Slot {} | {} txs | {:.2}ms",
                        self.processor.current_slot(),
                        tx_count,
                        processing_time.as_secs_f64() * 1000.0
                    );
                }
                last_log_slot = self.processor.current_slot();
            }

            // Warn if we're falling behind
            if processing_time > block_duration {
                tracing::warn!(
                    "Block {} took {:.2}ms (target: {}ms)",
                    self.processor.current_slot(),
                    processing_time.as_secs_f64() * 1000.0,
                    self.config.block_time_ms
                );
            }

            // Sleep for remaining time
            if let Some(sleep_time) = block_duration.checked_sub(processing_time) {
                std::thread::sleep(sleep_time);
            }
        }

        tracing::info!("Block producer stopped at slot {}", self.processor.current_slot());
    }

    /// Run the block producer asynchronously (tokio)
    pub async fn run_async(mut self) {
        self.running.store(true, Ordering::SeqCst);

        let block_duration = Duration::from_millis(self.config.block_time_ms);
        let mut interval = tokio::time::interval(block_duration);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut pending_txs: Vec<SanitizedTransaction> = Vec::with_capacity(self.config.max_txs_per_block);

        tracing::info!(
            "Block producer started ({}ms blocks, {}Hz)",
            self.config.block_time_ms,
            1000 / self.config.block_time_ms
        );

        while self.running.load(Ordering::SeqCst) {
            interval.tick().await;
            let tick_start = Instant::now();

            // Drain transaction queue
            loop {
                match self.tx_receiver.try_recv() {
                    Ok(tx) => {
                        pending_txs.push(tx);
                        if pending_txs.len() >= self.config.max_txs_per_block {
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        tracing::warn!("Transaction channel disconnected");
                        return;
                    }
                }
            }

            // Process transactions
            let mut transaction_results = Vec::new();
            let mut modified_accounts = Vec::new();

            if !pending_txs.is_empty() {
                let results = self.processor.process_transactions(&pending_txs);

                for result in results {
                    if result.success {
                        modified_accounts.extend(result.modified_accounts.clone());
                    }
                    transaction_results.push(result);
                }
            }

            let tx_count = pending_txs.len();
            pending_txs.clear();

            // Advance slot
            self.processor.advance_slot();

            let processing_time = tick_start.elapsed();

            // Create and broadcast block update
            let update = BlockUpdate {
                slot: self.processor.current_slot(),
                blockhash: self.processor.current_blockhash(),
                transaction_count: tx_count,
                modified_accounts,
                transaction_results,
                processing_time_us: processing_time.as_micros() as u64,
            };

            let _ = self.update_sender.send(update);

            // Warn if we're falling behind
            if processing_time > block_duration {
                tracing::warn!(
                    "Block {} took {:.2}ms (target: {}ms)",
                    self.processor.current_slot(),
                    processing_time.as_secs_f64() * 1000.0,
                    self.config.block_time_ms
                );
            }
        }

        tracing::info!("Block producer stopped");
    }
}
