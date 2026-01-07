//! Shared types for L1/L2 integration
//!
//! This crate contains type definitions that mirror the L1 Solana Program
//! (D:/Code/Solana-Program) for deserialization purposes.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::pubkey::Pubkey;

/// Maximum number of tokens a user can hold (matches L1)
pub const MAX_TOKENS: usize = 25;

/// Precision for percentage calculations (matches L1)
pub const PERCENTAGE_PRECISION: u128 = 1_000_000;

/// Token balance entry (matches L1 User.token_balance.tokens)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct Token {
    pub mint: Pubkey,
    pub amount: u128,
}

/// Token balance container (matches L1 User.token_balance)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct TokenBalance {
    pub tokens: [Token; MAX_TOKENS],
    pub count: u8,
}

impl Default for TokenBalance {
    fn default() -> Self {
        Self {
            tokens: std::array::from_fn(|_| Token::default()),
            count: 0,
        }
    }
}

/// In-game state (matches L1 User.in_game)
///
/// Note: This does NOT gate L2 world entry.
/// - `state = true` means player is in a dangerous PVP zone
/// - When true: items can be lost on death, admin can transfer items
/// - When true: user cannot transfer items out of userPDA (locked)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct InGame {
    pub state: bool,
    pub init_ts: i64,
}

/// L1 User account structure (for deserialization from L1)
///
/// This mirrors the User struct from D:/Code/Solana-Program/program/src/state/user.rs
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct L1User {
    pub game: Pubkey,
    pub pubkey: Pubkey,
    pub authority: Pubkey,
    pub game_key: Pubkey,
    pub bank: Pubkey,
    pub init_ts: i64,
    pub last_key_update_ts: i64,
    pub in_game: InGame,
    pub token_balance: TokenBalance,
    pub total_deposits: u64,
    pub total_withdraws: u64,
    pub total_fee_paid: u64,
    pub bump: u8,
}

/// L1 Bank account structure (for deserialization from L1)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct L1Bank {
    pub pubkey: Pubkey,
    pub authority: Pubkey,
    pub game: Pubkey,
    pub token_balance: TokenBalance,
    pub bump: u8,
}

/// Derive L1 User PDA
pub fn derive_l1_user_pda(game: &Pubkey, authority: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"user", game.as_ref(), authority.as_ref()], program_id)
}

/// Derive L1 Bank PDA
pub fn derive_l1_bank_pda(game: &Pubkey, authority: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"bank", game.as_ref(), authority.as_ref()], program_id)
}

/// Placeholder weapon stats for future L1 inventory integration
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct WeaponStats {
    pub damage: u16,
    pub range: u16,
    pub attack_speed: u8,
}

/// Placeholder armor stats for future L1 inventory integration
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct ArmorStats {
    pub defense: u16,
    pub durability: u16,
}
