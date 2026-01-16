//! Persistence Layer for L2 State
//!
//! Uses sled embedded database to persist account state across restarts.
//! State is saved periodically and on shutdown.

use serde::{Deserialize, Serialize};
use sled::Db;
use solana_sdk::{
    account::AccountSharedData,
    clock::Slot,
    hash::Hash,
    pubkey::Pubkey,
};
use std::path::Path;

/// Metadata about the chain state
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChainMetadata {
    /// Current slot (block height)
    pub slot: Slot,
    /// Current blockhash
    pub blockhash: [u8; 32],
    /// Current epoch
    pub epoch: u64,
    /// Total accounts stored
    pub account_count: u64,
    /// Last save timestamp
    pub last_save_ts: i64,
}

impl Default for ChainMetadata {
    fn default() -> Self {
        Self {
            slot: 0,
            blockhash: [0u8; 32],
            epoch: 0,
            account_count: 0,
            last_save_ts: 0,
        }
    }
}

/// Persistent storage for L2 state
pub struct PersistentStore {
    /// Sled database instance
    db: Db,
    /// Accounts tree
    accounts: sled::Tree,
    /// Account slots tree (tracks when each account was modified)
    account_slots: sled::Tree,
    /// Metadata tree
    metadata: sled::Tree,
}

impl PersistentStore {
    /// Open or create a persistent store at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let db = sled::open(&path)?;
        let accounts = db.open_tree("accounts")?;
        let account_slots = db.open_tree("account_slots")?;
        let metadata = db.open_tree("metadata")?;

        tracing::info!("Opened persistent store at {:?}", path.as_ref());

        Ok(Self {
            db,
            accounts,
            account_slots,
            metadata,
        })
    }

    /// Store an account
    pub fn store_account(&self, pubkey: &Pubkey, account: &AccountSharedData, slot: Slot) -> anyhow::Result<()> {
        // Serialize account using bincode
        let account_bytes = bincode::serialize(account)?;
        self.accounts.insert(pubkey.as_ref(), account_bytes)?;

        // Store the slot
        let slot_bytes = slot.to_le_bytes();
        self.account_slots.insert(pubkey.as_ref(), &slot_bytes)?;

        Ok(())
    }

    /// Get an account
    pub fn get_account(&self, pubkey: &Pubkey) -> anyhow::Result<Option<AccountSharedData>> {
        match self.accounts.get(pubkey.as_ref())? {
            Some(bytes) => {
                let account: AccountSharedData = bincode::deserialize(&bytes)?;
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    /// Get account with slot
    pub fn get_account_with_slot(&self, pubkey: &Pubkey) -> anyhow::Result<Option<(AccountSharedData, Slot)>> {
        let account = match self.get_account(pubkey)? {
            Some(a) => a,
            None => return Ok(None),
        };

        let slot = match self.account_slots.get(pubkey.as_ref())? {
            Some(bytes) => {
                let arr: [u8; 8] = bytes.as_ref().try_into().unwrap_or([0u8; 8]);
                Slot::from_le_bytes(arr)
            }
            None => 0,
        };

        Ok(Some((account, slot)))
    }

    /// Remove an account
    pub fn remove_account(&self, pubkey: &Pubkey) -> anyhow::Result<()> {
        self.accounts.remove(pubkey.as_ref())?;
        self.account_slots.remove(pubkey.as_ref())?;
        Ok(())
    }

    /// Get all accounts (for loading into memory)
    pub fn get_all_accounts(&self) -> anyhow::Result<Vec<(Pubkey, AccountSharedData, Slot)>> {
        let mut accounts = Vec::new();

        for result in self.accounts.iter() {
            let (key, value) = result?;

            // Parse pubkey
            let pubkey_bytes: [u8; 32] = key.as_ref().try_into()
                .map_err(|_| anyhow::anyhow!("Invalid pubkey length"))?;
            let pubkey = Pubkey::new_from_array(pubkey_bytes);

            // Deserialize account
            let account: AccountSharedData = bincode::deserialize(&value)?;

            // Get slot
            let slot = match self.account_slots.get(&key)? {
                Some(bytes) => {
                    let arr: [u8; 8] = bytes.as_ref().try_into().unwrap_or([0u8; 8]);
                    Slot::from_le_bytes(arr)
                }
                None => 0,
            };

            accounts.push((pubkey, account, slot));
        }

        Ok(accounts)
    }

    /// Save chain metadata
    pub fn save_metadata(&self, metadata: &ChainMetadata) -> anyhow::Result<()> {
        let bytes = bincode::serialize(metadata)?;
        self.metadata.insert("chain", bytes)?;
        Ok(())
    }

    /// Load chain metadata
    pub fn load_metadata(&self) -> anyhow::Result<Option<ChainMetadata>> {
        match self.metadata.get("chain")? {
            Some(bytes) => {
                let metadata: ChainMetadata = bincode::deserialize(&bytes)?;
                Ok(Some(metadata))
            }
            None => Ok(None),
        }
    }

    /// Flush all pending writes to disk
    pub fn flush(&self) -> anyhow::Result<()> {
        self.db.flush()?;
        Ok(())
    }

    /// Get the number of stored accounts
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Clear all data (for testing)
    pub fn clear(&self) -> anyhow::Result<()> {
        self.accounts.clear()?;
        self.account_slots.clear()?;
        self.metadata.clear()?;
        Ok(())
    }
}

/// Extension trait to add persistence to AccountStore
pub trait AccountStorePersistence {
    /// Save all accounts to persistent storage
    fn save_to_disk(&self, store: &PersistentStore) -> anyhow::Result<usize>;

    /// Load all accounts from persistent storage
    fn load_from_disk(&self, store: &PersistentStore) -> anyhow::Result<usize>;
}

impl AccountStorePersistence for crate::AccountStore {
    fn save_to_disk(&self, store: &PersistentStore) -> anyhow::Result<usize> {
        let mut count = 0;

        for pubkey in self.get_all_pubkeys() {
            if let Some((account, slot)) = self.get_account_with_slot(&pubkey) {
                store.store_account(&pubkey, &account, slot)?;
                count += 1;
            }
        }

        store.flush()?;
        tracing::info!("Saved {} accounts to disk", count);

        Ok(count)
    }

    fn load_from_disk(&self, store: &PersistentStore) -> anyhow::Result<usize> {
        let accounts = store.get_all_accounts()?;
        let count = accounts.len();

        for (pubkey, account, slot) in accounts {
            self.store_account(pubkey, account, slot);
        }

        tracing::info!("Loaded {} accounts from disk", count);

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::{Account, ReadableAccount};
    use tempfile::tempdir;

    #[test]
    fn test_store_and_load_account() {
        let dir = tempdir().unwrap();
        let store = PersistentStore::open(dir.path()).unwrap();

        let pubkey = Pubkey::new_unique();
        let account = AccountSharedData::from(Account {
            lamports: 1000,
            data: vec![1, 2, 3, 4],
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        });

        store.store_account(&pubkey, &account, 42).unwrap();
        store.flush().unwrap();

        let (loaded, slot) = store.get_account_with_slot(&pubkey).unwrap().unwrap();
        assert_eq!(loaded.lamports(), 1000);
        assert_eq!(loaded.data(), &[1, 2, 3, 4]);
        assert_eq!(slot, 42);
    }

    #[test]
    fn test_metadata() {
        let dir = tempdir().unwrap();
        let store = PersistentStore::open(dir.path()).unwrap();

        let metadata = ChainMetadata {
            slot: 1000,
            blockhash: [42u8; 32],
            epoch: 5,
            account_count: 100,
            last_save_ts: 12345,
        };

        store.save_metadata(&metadata).unwrap();

        let loaded = store.load_metadata().unwrap().unwrap();
        assert_eq!(loaded.slot, 1000);
        assert_eq!(loaded.epoch, 5);
    }
}
