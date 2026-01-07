//! TransactionProcessingCallback implementation for L2
//!
//! This is the bridge between the SVM and our account storage.
//! The SVM calls these methods to load accounts during transaction processing.

use crate::account_store::AccountStore;
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount},
    pubkey::Pubkey,
};
use solana_svm::transaction_processing_callback::TransactionProcessingCallback;
use std::sync::Arc;

/// L2 Account Loader - implements TransactionProcessingCallback
///
/// This struct provides the SVM with access to our account storage.
/// It's the critical bridge that allows the SVM to read accounts.
pub struct L2AccountLoader {
    /// Reference to the account store
    account_store: Arc<AccountStore>,
}

impl L2AccountLoader {
    /// Create a new account loader
    pub fn new(account_store: Arc<AccountStore>) -> Self {
        Self { account_store }
    }

    /// Get a reference to the underlying account store
    pub fn account_store(&self) -> &AccountStore {
        &self.account_store
    }
}

impl TransactionProcessingCallback for L2AccountLoader {
    /// Get account data for a pubkey
    ///
    /// This is called by the SVM during transaction loading to retrieve
    /// account data for all accounts referenced in a transaction.
    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.account_store.get_account(pubkey)
    }

    /// Check if an account is owned by one of the given programs
    ///
    /// Returns the index of the matching owner, or None if no match.
    /// This is used for account validation during transaction processing.
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        let account_data = self.account_store.get_account(account)?;
        let owner = account_data.owner();
        owners.iter().position(|candidate| candidate == owner)
    }

    /// Add a builtin account
    ///
    /// This is called during processor initialization to register builtin
    /// programs (system program, BPF loaders, etc.)
    fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
        tracing::debug!("Adding builtin account: {} ({})", name, program_id);
        // Builtins are pre-loaded during processor initialization
        // The actual account data is set up in the processor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    #[test]
    fn test_get_account() {
        let store = Arc::new(AccountStore::new());
        let loader = L2AccountLoader::new(store.clone());

        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::from(Account {
            lamports: 500,
            data: vec![],
            owner: solana_sdk::system_program::id(),
            executable: false,
            rent_epoch: 0,
        });

        store.store_account(pubkey, account, 0);

        let retrieved = loader.get_account_shared_data(&pubkey).unwrap();
        assert_eq!(retrieved.lamports(), 500);
    }

    #[test]
    fn test_account_matches_owners() {
        let store = Arc::new(AccountStore::new());
        let loader = L2AccountLoader::new(store.clone());

        let owner1 = Pubkey::new_unique();
        let owner2 = Pubkey::new_unique();
        let owner3 = Pubkey::new_unique();

        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::from(Account {
            lamports: 100,
            data: vec![],
            owner: owner2,
            executable: false,
            rent_epoch: 0,
        });

        store.store_account(pubkey, account, 0);

        // Should find owner2 at index 1
        let owners = vec![owner1, owner2, owner3];
        assert_eq!(loader.account_matches_owners(&pubkey, &owners), Some(1));

        // Should not find a match
        let other_owners = vec![owner1, owner3];
        assert_eq!(loader.account_matches_owners(&pubkey, &other_owners), None);
    }

    #[test]
    fn test_missing_account() {
        let store = Arc::new(AccountStore::new());
        let loader = L2AccountLoader::new(store);

        let pubkey = Pubkey::new_unique();
        assert!(loader.get_account_shared_data(&pubkey).is_none());
    }
}
