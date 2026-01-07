//! Subscription Manager - Manages WebSocket subscriptions
//!
//! Handles account subscriptions and broadcasts updates to subscribers.

use dashmap::DashMap;
use solana_sdk::{account::AccountSharedData, pubkey::Pubkey};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::broadcast;

/// Subscription ID
pub type SubscriptionId = u64;

/// Account update notification
#[derive(Clone, Debug)]
pub struct AccountNotification {
    pub subscription_id: SubscriptionId,
    pub pubkey: Pubkey,
    pub slot: u64,
    pub account: AccountSharedData,
}

/// Subscription entry
#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: SubscriptionId,
    pub pubkey: Pubkey,
    pub sender: broadcast::Sender<AccountNotification>,
}

/// Manages WebSocket subscriptions
pub struct SubscriptionManager {
    /// Active subscriptions by ID
    subscriptions: DashMap<SubscriptionId, Subscription>,
    /// Subscriptions by pubkey for efficient lookup
    pubkey_subscriptions: DashMap<Pubkey, Vec<SubscriptionId>>,
    /// Next subscription ID
    next_id: AtomicU64,
}

impl SubscriptionManager {
    /// Create a new subscription manager
    pub fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
            pubkey_subscriptions: DashMap::new(),
            next_id: AtomicU64::new(1),
        }
    }

    /// Subscribe to account updates
    pub fn subscribe_account(
        &self,
        pubkey: Pubkey,
    ) -> (SubscriptionId, broadcast::Receiver<AccountNotification>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = broadcast::channel(64);

        let subscription = Subscription {
            id,
            pubkey,
            sender,
        };

        self.subscriptions.insert(id, subscription);

        // Add to pubkey index
        self.pubkey_subscriptions
            .entry(pubkey)
            .or_default()
            .push(id);

        tracing::debug!("Created subscription {} for account {}", id, pubkey);

        (id, receiver)
    }

    /// Unsubscribe from account updates
    pub fn unsubscribe(&self, subscription_id: SubscriptionId) -> bool {
        if let Some((_, sub)) = self.subscriptions.remove(&subscription_id) {
            // Remove from pubkey index
            if let Some(mut subs) = self.pubkey_subscriptions.get_mut(&sub.pubkey) {
                subs.retain(|&id| id != subscription_id);
            }
            tracing::debug!("Removed subscription {}", subscription_id);
            true
        } else {
            false
        }
    }

    /// Notify subscribers of account update
    pub fn notify_account_update(&self, pubkey: &Pubkey, slot: u64, account: &AccountSharedData) {
        if let Some(sub_ids) = self.pubkey_subscriptions.get(pubkey) {
            for &sub_id in sub_ids.iter() {
                if let Some(sub) = self.subscriptions.get(&sub_id) {
                    let notification = AccountNotification {
                        subscription_id: sub_id,
                        pubkey: *pubkey,
                        slot,
                        account: account.clone(),
                    };

                    // Ignore send errors (subscriber might have disconnected)
                    let _ = sub.sender.send(notification);
                }
            }
        }
    }

    /// Notify subscribers of multiple account updates
    pub fn notify_account_updates(&self, updates: &[(Pubkey, AccountSharedData)], slot: u64) {
        for (pubkey, account) in updates {
            self.notify_account_update(pubkey, slot, account);
        }
    }

    /// Get subscription count
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Check if a subscription exists
    pub fn has_subscription(&self, subscription_id: SubscriptionId) -> bool {
        self.subscriptions.contains_key(&subscription_id)
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    #[tokio::test]
    async fn test_subscribe_and_notify() {
        let manager = SubscriptionManager::new();
        let pubkey = Pubkey::new_unique();

        let (sub_id, mut receiver) = manager.subscribe_account(pubkey);
        assert_eq!(sub_id, 1);

        let account = AccountSharedData::from(Account {
            lamports: 100,
            data: vec![],
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        });

        manager.notify_account_update(&pubkey, 1, &account);

        let notification = receiver.recv().await.unwrap();
        assert_eq!(notification.subscription_id, sub_id);
        assert_eq!(notification.pubkey, pubkey);
    }

    #[test]
    fn test_unsubscribe() {
        let manager = SubscriptionManager::new();
        let pubkey = Pubkey::new_unique();

        let (sub_id, _) = manager.subscribe_account(pubkey);
        assert!(manager.has_subscription(sub_id));

        manager.unsubscribe(sub_id);
        assert!(!manager.has_subscription(sub_id));
    }
}
