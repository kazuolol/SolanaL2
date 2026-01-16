//! L2 Transaction Processor
//!
//! Wraps the solana-svm TransactionBatchProcessor to provide
//! transaction execution for the L2 gaming chain.

use crate::{account_store::AccountStore, callback::L2AccountLoader};
use solana_compute_budget::compute_budget::ComputeBudget;
use solana_program_runtime::{
    invoke_context::BuiltinFunctionWithContext,
    loaded_programs::{BlockRelation, ForkGraph, ProgramCacheEntry, ProgramCacheEntryType},
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
        let result = if a == b {
            BlockRelation::Equal
        } else if a < b {
            BlockRelation::Ancestor
        } else {
            BlockRelation::Descendant
        };
        tracing::info!("ForkGraph::relationship({}, {}) = {:?}", a, b, result);
        result
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
        eprintln!("[L2Processor] Creating new L2Processor...");

        // Pre-initialize the RBPF runtime environment key.
        // The solana_rbpf library uses a random key for pointer obfuscation in invoke_function.
        // Initializing it early prevents potential blocking during first transaction execution.
        eprintln!("[L2Processor] Initializing RBPF runtime environment key...");
        let key = solana_program_runtime::solana_rbpf::vm::get_runtime_environment_key();
        eprintln!("[L2Processor] RBPF runtime environment key initialized: {}", key);

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

        // Set up the program cache with proper environments and fork graph
        {
            let mut program_cache = processor.program_cache.write().unwrap();
            program_cache.set_fork_graph(Arc::downgrade(&fork_graph));

            // Initialize proper program runtime environments with syscalls
            // The default ProgramRuntimeEnvironments has empty loaders which can cause
            // issues during builtin execution. We need properly configured environments.
            let compute_budget = ComputeBudget::default();
            match solana_bpf_loader_program::syscalls::create_program_runtime_environment_v1(
                &feature_set,
                &compute_budget,
                false, // reject_deployment_of_broken_elfs
                false, // debugging_features
            ) {
                Ok(runtime_v1) => {
                    program_cache.environments.program_runtime_v1 = Arc::new(runtime_v1);
                    tracing::info!("Initialized program_runtime_v1 environment with syscalls");
                }
                Err(e) => {
                    tracing::warn!("Failed to create program_runtime_v1: {:?}, using default", e);
                }
            }

            // Also initialize v2 (doesn't return Result, always succeeds)
            let runtime_v2 = solana_bpf_loader_program::syscalls::create_program_runtime_environment_v2(
                &compute_budget,
                false, // debugging_features
            );
            program_cache.environments.program_runtime_v2 = Arc::new(runtime_v2);
            tracing::info!("Initialized program_runtime_v2 environment");
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

        // Verify builtins are in cache immediately after registration
        // NOTE: get_flattened_entries() only returns Loaded entries, NOT Builtin entries!
        // We must use get_slot_versions_for_tests() to check if builtins are actually in the cache.
        {
            let cache = this.processor.program_cache.read().unwrap();
            let builtin_ids = this.processor.builtin_program_ids.read().unwrap();
            tracing::info!("After registration: {} builtin IDs registered", builtin_ids.len());

            for id in builtin_ids.iter() {
                let versions = cache.get_slot_versions_for_tests(id);
                if versions.is_empty() {
                    tracing::error!("BUILTIN {} NOT IN CACHE!", id);
                } else {
                    tracing::info!("Builtin {} has {} version(s) in cache:", id, versions.len());
                    for v in versions {
                        let type_name = match &v.program {
                            ProgramCacheEntryType::Builtin(_) => "Builtin",
                            ProgramCacheEntryType::Loaded(_) => "Loaded",
                            ProgramCacheEntryType::Unloaded(_) => "Unloaded",
                            ProgramCacheEntryType::FailedVerification(_) => "FailedVerification",
                            ProgramCacheEntryType::Closed => "Closed",
                            ProgramCacheEntryType::DelayVisibility => "DelayVisibility",
                        };
                        tracing::info!("  - deployment_slot={}, effective_slot={}, type={}",
                            v.deployment_slot, v.effective_slot, type_name);
                    }
                }
            }
        }

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
        tracing::info!("Registering builtin program: {} ({})", name, program_id);

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

        // Verify the builtin account exists in the store
        if let Some(account) = self.account_store.get_account(&program_id) {
            use solana_sdk::account::ReadableAccount;
            tracing::info!(
                "  Builtin account verified: owner={}, executable={}, len={}",
                account.owner(), account.executable(), account.data().len()
            );
        } else {
            tracing::error!("  BUILTIN ACCOUNT NOT FOUND IN STORE!");
        }
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
        // IMPORTANT: limit_to_load_programs = false allows loading programs from accounts
        // If true, it only uses pre-loaded programs (might cause issues)
        let config = TransactionProcessingConfig {
            compute_budget: Some(ComputeBudget::default()),
            log_messages_bytes_limit: Some(10_000),
            limit_to_load_programs: false, // Allow loading programs dynamically
            recording_config: ExecutionRecordingConfig {
                enable_log_recording: true,
                enable_return_data_recording: true,
                enable_cpi_recording: false,
            },
            ..Default::default()
        };
        tracing::info!("SVM: Config - limit_to_load_programs={}", config.limit_to_load_programs);

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
        tracing::info!("SVM: Starting load_and_execute_sanitized_transactions for {} txs", transactions.len());
        tracing::info!("SVM: Current slot = {}, epoch = {}", self.current_slot, self.current_epoch);

        // Log program cache state for debugging
        // NOTE: get_flattened_entries only returns Loaded entries, not Builtin entries!
        // We verify builtins at startup using get_slot_versions_for_tests instead.
        {
            let cache = self.processor.program_cache.read().unwrap();
            let builtin_ids = self.processor.builtin_program_ids.read().unwrap();
            tracing::info!("SVM: {} builtins registered, verifying cache state...", builtin_ids.len());

            let mut all_builtins_present = true;
            for id in builtin_ids.iter() {
                let versions = cache.get_slot_versions_for_tests(id);
                if versions.is_empty() {
                    tracing::error!("SVM: BUILTIN {} MISSING FROM CACHE!", id);
                    all_builtins_present = false;
                }
            }
            if all_builtins_present {
                tracing::info!("SVM: All {} builtins present in cache", builtin_ids.len());
            }
        }

        // CRITICAL: Fill the sysvar cache from account store before execution
        // The invoke_context.get_sysvar_cache().get_clock() will fail if this isn't done
        self.processor.fill_missing_sysvar_cache_entries(&callback);
        tracing::info!("SVM: Filled sysvar cache entries");

        tracing::info!("SVM: Calling load_and_execute_sanitized_transactions...");
        eprintln!("[SVM] ABOUT TO CALL load_and_execute_sanitized_transactions");
        eprintln!("[SVM] slot={}, epoch={}", self.current_slot, self.current_epoch);

        // Use catch_unwind to see if there's a panic
        let output_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.processor.load_and_execute_sanitized_transactions(
            &callback,
            transactions,
            check_results,
            &environment,
            &config,
        )
        }));

        let output = match output_result {
            Ok(o) => {
                eprintln!("[SVM] RETURNED FROM load_and_execute_sanitized_transactions - SUCCESS");
                o
            }
            Err(e) => {
                eprintln!("[SVM] PANIC in load_and_execute_sanitized_transactions: {:?}", e);
                panic!("SVM panicked: {:?}", e);
            }
        };
        tracing::info!("SVM: Returned from load_and_execute_sanitized_transactions");
        tracing::info!("SVM: Completed successfully with {} results", output.processing_results.len());

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

                            tracing::info!(
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
                    // Log detailed error info including accounts referenced
                    let account_keys: Vec<_> = tx.message().account_keys().iter().collect();
                    tracing::error!(
                        "Transaction {} failed: {:?}",
                        signature, e
                    );
                    tracing::error!("  Accounts referenced ({}):", account_keys.len());
                    for (i, key) in account_keys.iter().enumerate() {
                        let exists = self.account_store.account_exists(key);
                        let account = self.account_store.get_account(key);
                        let (owner, data_len, executable) = account
                            .as_ref()
                            .map(|a| {
                                use solana_sdk::account::ReadableAccount;
                                (a.owner().to_string(), a.data().len(), a.executable())
                            })
                            .unwrap_or(("N/A".to_string(), 0, false));
                        tracing::error!(
                            "    [{}] {} (exists={}, owner={}, len={}, exec={})",
                            i, key, exists, owner, data_len, executable
                        );
                    }
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

        // Update fork graph slot FIRST (needed for cache visibility)
        {
            let mut fg = self.fork_graph.write().unwrap();
            fg.set_slot(self.current_slot);
        }

        // Create new processor at current slot while preserving program cache
        // This is needed because the processor's internal slot field is used for cache lookups
        self.processor = self.processor.new_from(self.current_slot, self.current_epoch);

        // Re-attach fork graph to the new program cache
        {
            let mut program_cache = self.processor.program_cache.write().unwrap();
            program_cache.set_fork_graph(Arc::downgrade(&self.fork_graph));
        }

        // NOTE: We do NOT re-register builtins - they persist in the shared program cache
        // Builtins registered at slot 0 are visible at all future slots via ForkGraph

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
