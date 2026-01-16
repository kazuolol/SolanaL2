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
    ///
    /// For L2, we return a default zeroed account for missing accounts.
    /// This allows new accounts (like player PDAs) to be created on-the-fly
    /// during transaction execution without requiring explicit create_account
    /// instructions via the system program.
    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        eprintln!("[CALLBACK] get_account_shared_data called for: {}", pubkey);
        match self.account_store.get_account(pubkey) {
            Some(account) => {
                eprintln!("[CALLBACK] Account FOUND: {} (owner: {}, len: {}, exec: {})",
                    pubkey, account.owner(), account.data().len(), account.executable());
                tracing::info!("Account FOUND in store: {} (owner: {}, len: {}, exec: {})",
                    pubkey, account.owner(), account.data().len(), account.executable());
                Some(account)
            }
            None => {
                // Return a default account for missing accounts
                // This enables account creation during transaction execution
                // The account will be properly initialized by the program
                // and saved to the store after successful execution
                //
                // IMPORTANT: We differentiate between wallet addresses and PDAs:
                // - Wallets (on ed25519 curve): need 0 data bytes for fee payer validation
                //   and are owned by system_program
                // - PDAs (off curve): need space for program data and are owned by
                //   world_program so the program can write to them
                //
                // PDAs are specifically created to be OFF the curve, while regular
                // wallet keypairs generate addresses ON the curve.
                let is_pda = !pubkey.is_on_curve();
                let (data_len, owner) = if is_pda {
                    // PDAs are owned by world_program so it can write to them
                    // This is the key L2 design decision - gasless PDA creation
                    (world_program::state::WorldPlayer::LEN, world_program::id())
                } else {
                    // Wallets are system-owned with 0 data
                    (0, solana_sdk::system_program::id())
                };

                tracing::warn!(
                    "Account NOT in store, returning default: {} (is_pda={}, data_len={}, owner={})",
                    pubkey, is_pda, data_len, owner
                );
                Some(AccountSharedData::new(
                    1_000_000_000, // 1 SOL worth of lamports - ensures fee payer validation passes
                    data_len,
                    &owner,
                ))
            }
        }
    }

    /// Check if an account is owned by one of the given programs
    ///
    /// Returns the index of the matching owner, or None if no match.
    /// This is used for account validation during transaction processing.
    ///
    /// For missing accounts, we treat them as system-owned (consistent with
    /// get_account_shared_data returning system-owned default accounts).
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        let result = match self.account_store.get_account(account) {
            Some(account_data) => {
                let owner = account_data.owner();
                owners.iter().position(|candidate| candidate == owner)
            }
            None => {
                // For missing accounts, check if it's a PDA or wallet
                // PDAs are owned by world_program, wallets by system_program
                let is_pda = !account.is_on_curve();
                let expected_owner = if is_pda {
                    world_program::id()
                } else {
                    solana_sdk::system_program::id()
                };
                owners.iter().position(|candidate| candidate == &expected_owner)
            }
        };
        if result.is_none() {
            tracing::warn!(
                "account_matches_owners: {} NOT MATCHED against owners: {:?}",
                account, owners
            );
        }
        result
    }

    /// Add a builtin account
    ///
    /// This is called during processor initialization to register builtin
    /// programs (system program, BPF loaders, etc.)
    ///
    /// IMPORTANT: The SVM expects this to actually create the account in the store.
    /// Without this, the SVM may return AccountNotFound during transaction processing.
    fn add_builtin_account(&self, name: &str, program_id: &Pubkey) {
        tracing::info!("Adding builtin account: {} ({})", name, program_id);

        // Create executable account owned by native loader
        use solana_sdk::{account::Account, native_loader};
        let account = AccountSharedData::from(Account {
            lamports: 1,
            data: program_id.to_bytes().to_vec(),
            owner: native_loader::id(),
            executable: true,
            rent_epoch: 0,
        });

        self.account_store.store_account(*program_id, account, 0);
        tracing::debug!("Builtin account {} stored in account store", program_id);
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
    fn test_missing_wallet_returns_default_with_zero_data() {
        use solana_sdk::signature::{Keypair, Signer};

        let store = Arc::new(AccountStore::new());
        let loader = L2AccountLoader::new(store);

        // Use Keypair to get a real on-curve wallet address
        let pubkey = Keypair::new().pubkey();
        assert!(pubkey.is_on_curve(), "Keypair pubkey should be on-curve");

        let account = loader.get_account_shared_data(&pubkey);
        assert!(account.is_some());
        let account = account.unwrap();
        // Default accounts get 1 SOL to pass fee payer validation
        assert_eq!(account.lamports(), 1_000_000_000);
        // Wallets (on-curve) must have 0 data bytes for fee payer validation
        assert_eq!(account.data().len(), 0);
        // Wallets are owned by system_program
        assert_eq!(account.owner(), &solana_sdk::system_program::id());
    }

    #[test]
    fn test_missing_pda_returns_default_with_data_space() {
        let store = Arc::new(AccountStore::new());
        let loader = L2AccountLoader::new(store);

        // Create a PDA (off-curve address) using find_program_address
        let program_id = Pubkey::new_unique();
        let (pda, _bump) = Pubkey::find_program_address(&[b"test"], &program_id);

        let account = loader.get_account_shared_data(&pda);
        assert!(account.is_some());
        let account = account.unwrap();
        // Default accounts get 1 SOL
        assert_eq!(account.lamports(), 1_000_000_000);
        // PDAs (off-curve) get WorldPlayer::LEN bytes for program data
        assert_eq!(account.data().len(), world_program::state::WorldPlayer::LEN);
        // PDAs are owned by world_program so the program can write to them
        assert_eq!(account.owner(), &world_program::id());
    }
}
