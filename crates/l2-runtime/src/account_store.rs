//! In-memory account storage using DashMap for concurrent access

use dashmap::DashMap;
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount},
    clock::Slot,
    pubkey::Pubkey,
};
use std::sync::Arc;

/// Thread-safe in-memory account storage
///
/// Uses DashMap for lock-free concurrent reads and fine-grained write locks.
/// This is optimized for the 50-100 concurrent player target.
#[derive(Clone)]
pub struct AccountStore {
    /// Main account storage
    accounts: Arc<DashMap<Pubkey, AccountSharedData>>,
    /// Track which slot each account was last modified
    account_slots: Arc<DashMap<Pubkey, Slot>>,
}

impl AccountStore {
    /// Create a new empty account store
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(DashMap::new()),
            account_slots: Arc::new(DashMap::new()),
        }
    }

    /// Get an account by pubkey
    pub fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.accounts.get(pubkey).map(|r| r.value().clone())
    }

    /// Get an account with the slot it was last modified
    pub fn get_account_with_slot(&self, pubkey: &Pubkey) -> Option<(AccountSharedData, Slot)> {
        let account = self.accounts.get(pubkey)?;
        let slot = self.account_slots.get(pubkey).map(|s| *s).unwrap_or(0);
        Some((account.value().clone(), slot))
    }

    /// Store an account
    pub fn store_account(&self, pubkey: Pubkey, account: AccountSharedData, slot: Slot) {
        self.accounts.insert(pubkey, account);
        self.account_slots.insert(pubkey, slot);
    }

    /// Store multiple accounts atomically (best effort - not truly atomic)
    pub fn store_accounts(&self, accounts: Vec<(Pubkey, AccountSharedData)>, slot: Slot) {
        for (pubkey, account) in accounts {
            self.store_account(pubkey, account, slot);
        }
    }

    /// Check if an account exists
    pub fn account_exists(&self, pubkey: &Pubkey) -> bool {
        self.accounts.contains_key(pubkey)
    }

    /// Get account lamports (returns 0 if account doesn't exist)
    pub fn get_lamports(&self, pubkey: &Pubkey) -> u64 {
        self.accounts
            .get(pubkey)
            .map(|a| a.lamports())
            .unwrap_or(0)
    }

    /// Get all account pubkeys (for debugging/iteration)
    pub fn get_all_pubkeys(&self) -> Vec<Pubkey> {
        self.accounts.iter().map(|r| *r.key()).collect()
    }

    /// Get account count
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    /// Remove an account
    pub fn remove_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        self.account_slots.remove(pubkey);
        self.accounts.remove(pubkey).map(|(_, v)| v)
    }

    /// Clear all accounts (for testing)
    pub fn clear(&self) {
        self.accounts.clear();
        self.account_slots.clear();
    }

    /// Get accounts owned by a specific program
    pub fn get_program_accounts(&self, program_id: &Pubkey) -> Vec<(Pubkey, AccountSharedData)> {
        self.accounts
            .iter()
            .filter(|r| r.value().owner() == program_id)
            .map(|r| (*r.key(), r.value().clone()))
            .collect()
    }
}

impl Default for AccountStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    #[test]
    fn test_store_and_get() {
        let store = AccountStore::new();
        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::from(Account {
            lamports: 1000,
            data: vec![1, 2, 3],
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        });

        store.store_account(pubkey, account.clone(), 1);

        let retrieved = store.get_account(&pubkey).unwrap();
        assert_eq!(retrieved.lamports(), 1000);
        assert_eq!(retrieved.data(), &[1, 2, 3]);
    }

    #[test]
    fn test_get_with_slot() {
        let store = AccountStore::new();
        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::default();

        store.store_account(pubkey, account, 42);

        let (_, slot) = store.get_account_with_slot(&pubkey).unwrap();
        assert_eq!(slot, 42);
    }
}
