//! L2 Transaction Processor
//!
//! Wraps the solana-svm TransactionBatchProcessor to provide
//! transaction execution for the L2 gaming chain.

use crate::{account_store::AccountStore, callback::L2AccountLoader};
use solana_compute_budget::compute_budget::ComputeBudget;
use solana_program_runtime::{
    invoke_context::BuiltinFunctionWithContext,
    loaded_programs::{BlockRelation, ForkGraph, ProgramCacheEntry},
};
use solana_sdk::{
    account::{Account, AccountSharedData},
    bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable,
    clock::{Clock, Slot},
    epoch_schedule::EpochSchedule,
    feature_set::FeatureSet,
    fee::FeeStructure,
    hash::Hash,
    native_loader,
    pubkey::Pubkey,
    rent::Rent,
    signature::Signature,
    sysvar::{self, Sysvar, SysvarId},
    transaction::{SanitizedTransaction, TransactionError},
};
use solana_svm::{
    account_loader::{CheckedTransactionDetails, TransactionCheckResult},
    transaction_processor::{
        ExecutionRecordingConfig, LoadAndExecuteSanitizedTransactionsOutput,
        TransactionBatchProcessor, TransactionProcessingConfig, TransactionProcessingEnvironment,
    },
};
use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

/// Simple linear fork graph for L2 (no forks, just linear chain)
#[derive(Debug, Default, Clone)]
pub struct L2ForkGraph {
    current_slot: Slot,
}

impl L2ForkGraph {
    pub fn new() -> Self {
        Self { current_slot: 0 }
    }

    pub fn set_slot(&mut self, slot: Slot) {
        self.current_slot = slot;
    }
}

impl ForkGraph for L2ForkGraph {
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
        // In our linear L2 chain, all slots are on the same fork
        if a == b {
            BlockRelation::Equal
        } else if a < b {
            BlockRelation::Ancestor
        } else {
            BlockRelation::Descendant
        }
    }
}

/// Result of processing a single transaction
#[derive(Debug, Clone)]
pub struct TransactionResult {
    pub signature: Signature,
    pub slot: Slot,
    pub success: bool,
    pub error: Option<TransactionError>,
    pub logs: Vec<String>,
    pub modified_accounts: Vec<(Pubkey, AccountSharedData)>,
}

/// L2 Transaction Processor
///
/// Wraps TransactionBatchProcessor to provide a high-level interface
/// for processing transactions on the L2 gaming chain.
pub struct L2Processor {
    /// The underlying SVM transaction processor
    processor: TransactionBatchProcessor<L2ForkGraph>,
    /// Account storage
    account_store: Arc<AccountStore>,
    /// Current slot (block height)
    current_slot: Slot,
    /// Current epoch
    current_epoch: u64,
    /// Current blockhash
    current_blockhash: Hash,
    /// Feature set (all enabled for L2)
    feature_set: Arc<FeatureSet>,
    /// Builtin program IDs
    builtin_program_ids: HashSet<Pubkey>,
    /// Fork graph for program cache
    fork_graph: Arc<RwLock<L2ForkGraph>>,
}

impl L2Processor {
    /// Create a new L2 processor
    pub fn new(account_store: Arc<AccountStore>) -> Self {
        let slot = 0;
        let epoch = 0;

        // Initialize feature set with all features enabled
        let feature_set = Arc::new(FeatureSet::all_enabled());

        // Initialize builtin program IDs
        let builtin_program_ids: HashSet<Pubkey> = [
            solana_sdk::system_program::id(),
            native_loader::id(),
            bpf_loader::id(),
            bpf_loader_deprecated::id(),
            bpf_loader_upgradeable::id(),
            world_program::id(),
        ]
        .into_iter()
        .collect();

        // Set up builtin accounts in the store
        Self::setup_builtin_accounts(&account_store, &builtin_program_ids);

        // Set up sysvar accounts
        Self::setup_sysvar_accounts(&account_store, slot, epoch);

        // Create fork graph for L2 linear chain
        let fork_graph = Arc::new(RwLock::new(L2ForkGraph::new()));

        // Create the transaction processor (uninitialized, we'll set up program cache separately)
        let processor = TransactionBatchProcessor::<L2ForkGraph>::new_uninitialized(slot, epoch);

        // Set the fork graph in the program cache (required for program lookups)
        {
            let mut program_cache = processor.program_cache.write().unwrap();
            program_cache.set_fork_graph(Arc::downgrade(&fork_graph));
        }

        let mut this = Self {
            processor,
            account_store,
            current_slot: slot,
            current_epoch: epoch,
            current_blockhash: Hash::new_unique(),
            feature_set,
            builtin_program_ids,
            fork_graph,
        };

        // Register builtin programs
        this.register_builtins();

        this
    }

    /// Set up builtin program accounts
    fn setup_builtin_accounts(store: &AccountStore, builtin_ids: &HashSet<Pubkey>) {
        for program_id in builtin_ids {
            let account = AccountSharedData::from(Account {
                lamports: 1,
                data: program_id.to_bytes().to_vec(),
                owner: native_loader::id(),
                executable: true,
                rent_epoch: 0,
            });
            store.store_account(*program_id, account, 0);
        }
    }

    /// Set up sysvar accounts
    fn setup_sysvar_accounts(store: &AccountStore, slot: Slot, epoch: u64) {
        // Clock sysvar
        let clock = Clock {
            slot,
            epoch_start_timestamp: 0,
            epoch,
            leader_schedule_epoch: epoch,
            unix_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };
        Self::store_sysvar(store, &clock);

        // Rent sysvar
        let rent = Rent::default();
        Self::store_sysvar(store, &rent);

        // EpochSchedule sysvar
        let epoch_schedule = EpochSchedule::default();
        Self::store_sysvar(store, &epoch_schedule);

        // Recent blockhashes - simplified for L2
        // In production, this would track recent blockhashes
    }

    /// Store a sysvar account
    fn store_sysvar<T: Sysvar + SysvarId>(store: &AccountStore, sysvar: &T) {
        let data = bincode::serialize(sysvar).unwrap();
        let account = AccountSharedData::from(Account {
            lamports: 1,
            data,
            owner: sysvar::id(),
            executable: false,
            rent_epoch: 0,
        });
        store.store_account(T::id(), account, 0);
    }

    /// Register builtin programs with the processor
    fn register_builtins(&mut self) {
        // System program
        self.add_builtin(
            "system_program",
            solana_sdk::system_program::id(),
            solana_system_program::system_processor::Entrypoint::vm,
        );

        // BPF Loader
        self.add_builtin(
            "solana_bpf_loader_program",
            bpf_loader::id(),
            solana_bpf_loader_program::Entrypoint::vm,
        );

        // BPF Loader Upgradeable
        self.add_builtin(
            "solana_bpf_loader_upgradeable_program",
            bpf_loader_upgradeable::id(),
            solana_bpf_loader_program::Entrypoint::vm,
        );

        // World Program - L2 game logic with signature verification
        self.add_builtin(
            "world_program",
            world_program::id(),
            world_program::builtin::Entrypoint::vm,
        );
    }

    /// Add a builtin program
    fn add_builtin(
        &mut self,
        name: &str,
        program_id: Pubkey,
        entrypoint: BuiltinFunctionWithContext,
    ) {
        let builtin = ProgramCacheEntry::new_builtin(
            self.current_slot,
            name.len(),
            entrypoint,
        );

        // Use L2AccountLoader as the callback for adding builtins
        let callback = L2AccountLoader::new(self.account_store.clone());
        self.processor.add_builtin(
            &callback,
            program_id,
            name,
            builtin,
        );
    }

    /// Process a batch of transactions
    pub fn process_transactions(
        &mut self,
        transactions: &[SanitizedTransaction],
    ) -> Vec<TransactionResult> {
        if transactions.is_empty() {
            return vec![];
        }

        let callback = L2AccountLoader::new(self.account_store.clone());

        // Fee structure for gasless transactions
        let fee_structure = FeeStructure::default();

        // Set up processing environment
        let environment = TransactionProcessingEnvironment {
            blockhash: self.current_blockhash,
            epoch_total_stake: Some(1_000_000_000), // Single validator has all stake
            epoch_vote_accounts: None,
            feature_set: self.feature_set.clone(),
            fee_structure: Some(&fee_structure),
            lamports_per_signature: 0, // Gasless transactions
            rent_collector: None,
        };

        // Set up processing config
        let config = TransactionProcessingConfig {
            compute_budget: Some(ComputeBudget::default()),
            log_messages_bytes_limit: Some(10_000),
            recording_config: ExecutionRecordingConfig {
                enable_log_recording: true,
                enable_return_data_recording: true,
                enable_cpi_recording: false,
            },
            ..Default::default()
        };

        // Create check results (all transactions are valid - already sanitized)
        // For gasless L2, we use 0 fees
        let check_results: Vec<TransactionCheckResult> = transactions
            .iter()
            .map(|_| Ok(CheckedTransactionDetails {
                nonce: None,
                lamports_per_signature: 0, // Gasless transactions
            }))
            .collect();

        // Process the batch
        let output = self.processor.load_and_execute_sanitized_transactions(
            &callback,
            transactions,
            check_results,
            &environment,
            &config,
        );

        // Convert results and update account store
        self.process_output(transactions, output)
    }

    /// Process the output from transaction execution
    fn process_output(
        &self,
        transactions: &[SanitizedTransaction],
        output: LoadAndExecuteSanitizedTransactionsOutput,
    ) -> Vec<TransactionResult> {
        use solana_svm::transaction_processing_result::ProcessedTransaction;

        let mut results = Vec::with_capacity(transactions.len());

        for (tx, result) in transactions
            .iter()
            .zip(output.processing_results.into_iter())
        {
            let signature = *tx.signature();

            match result {
                Ok(processed) => {
                    // Extract execution details from the processed transaction
                    let (success, error, logs) = if let Some(exec_details) = processed.execution_details() {
                        let logs = exec_details
                            .log_messages
                            .clone()
                            .unwrap_or_default();

                        let (success, error) = match &exec_details.status {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(e.clone())),
                        };

                        (success, error, logs)
                    } else {
                        (true, None, vec![])
                    };

                    // Extract and store modified accounts
                    let mut modified_accounts = Vec::new();

                    if success {
                        // Get accounts from the executed transaction
                        if let ProcessedTransaction::Executed(executed) = &processed {
                            // The loaded_transaction.accounts contains (Pubkey, AccountSharedData) tuples
                            // Write each modified account back to the store
                            for (pubkey, account) in &executed.loaded_transaction.accounts {
                                // Store the account in our account store
                                self.account_store.store_account(
                                    *pubkey,
                                    account.clone(),
                                    self.current_slot,
                                );
                                modified_accounts.push((*pubkey, account.clone()));
                            }

                            tracing::debug!(
                                "Transaction {} succeeded: {} accounts modified",
                                signature,
                                modified_accounts.len()
                            );
                        }
                    }

                    results.push(TransactionResult {
                        signature,
                        slot: self.current_slot,
                        success,
                        error,
                        logs,
                        modified_accounts,
                    });
                }
                Err(e) => {
                    tracing::debug!("Transaction {} failed: {:?}", signature, e);
                    results.push(TransactionResult {
                        signature,
                        slot: self.current_slot,
                        success: false,
                        error: Some(e),
                        logs: vec![],
                        modified_accounts: vec![],
                    });
                }
            }
        }

        results
    }

    /// Advance to the next slot
    pub fn advance_slot(&mut self) {
        self.current_slot += 1;
        self.current_blockhash = Hash::new_unique();

        // Update clock sysvar
        let clock = Clock {
            slot: self.current_slot,
            epoch_start_timestamp: 0,
            epoch: self.current_epoch,
            leader_schedule_epoch: self.current_epoch,
            unix_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };
        Self::store_sysvar(&self.account_store, &clock);

        tracing::trace!("Advanced to slot {}", self.current_slot);
    }

    /// Get current slot
    pub fn current_slot(&self) -> Slot {
        self.current_slot
    }

    /// Get current blockhash
    pub fn current_blockhash(&self) -> Hash {
        self.current_blockhash
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Get reference to account store
    pub fn account_store(&self) -> &AccountStore {
        &self.account_store
    }

    /// Load a program (BPF .so file) into the cache
    pub fn load_program(&mut self, program_id: Pubkey, program_data: Vec<u8>) -> anyhow::Result<()> {
        // Create program account
        let program_account = AccountSharedData::from(Account {
            lamports: 1,
            data: program_data.clone(),
            owner: bpf_loader_upgradeable::id(),
            executable: true,
            rent_epoch: 0,
        });

        self.account_store
            .store_account(program_id, program_account, self.current_slot);

        tracing::info!("Loaded program {} ({} bytes)", program_id, program_data.len());

        Ok(())
    }
}

impl Default for L2Processor {
    fn default() -> Self {
        Self::new(Arc::new(AccountStore::new()))
    }
}
