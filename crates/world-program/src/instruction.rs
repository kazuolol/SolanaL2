//! World Program Instructions

use borsh::{BorshDeserialize, BorshSerialize};
use crate::state::{MovementInput, MovementInput3D, WeaponStats};

/// World program instructions
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub enum WorldInstruction {
    /// Initialize a new world
    ///
    /// Accounts:
    /// 0. `[writable]` World config account (PDA)
    /// 1. `[signer]` Authority (world admin)
    /// 2. `[signer, writable]` Payer
    /// 3. `[]` System program
    InitializeWorld {
        /// World name (max 32 bytes)
        name: [u8; 32],
        /// World width in units
        width: u32,
        /// World height in units
        height: u32,
        /// Maximum players
        max_players: u16,
    },

    /// Join the world (create player account)
    ///
    /// Accounts:
    /// 0. `[]` World config account
    /// 1. `[writable]` World player account (PDA)
    /// 2. `[signer]` Player authority (wallet)
    /// 3. `[signer, writable]` Payer
    /// 4. `[]` System program
    JoinWorld {
        /// Player name (max 16 bytes)
        name: [u8; 16],
    },

    /// Move player
    ///
    /// Accounts:
    /// 0. `[]` World config account
    /// 1. `[writable]` World player account
    /// 2. `[signer]` Player authority
    MovePlayer {
        /// Movement input
        input: MovementInput,
    },

    /// Attack another player
    ///
    /// Accounts:
    /// 0. `[]` World config account
    /// 1. `[writable]` Attacker player account
    /// 2. `[writable]` Target player account
    /// 3. `[signer]` Attacker authority
    Attack {
        /// Optional weapon stats from L1 (uses default if None)
        weapon_stats: Option<WeaponStats>,
    },

    /// Heal self
    ///
    /// Accounts:
    /// 0. `[]` World config account
    /// 1. `[writable]` Player account
    /// 2. `[signer]` Player authority
    Heal {
        /// Heal amount (0 = use default)
        amount: u16,
    },

    /// Leave the world (close player account)
    ///
    /// Accounts:
    /// 0. `[writable]` World config account
    /// 1. `[writable]` World player account
    /// 2. `[signer]` Player authority
    /// 3. `[writable]` Rent destination
    LeaveWorld,

    /// Update world config (admin only)
    ///
    /// Accounts:
    /// 0. `[writable]` World config account
    /// 1. `[signer]` World authority
    UpdateWorld {
        /// New max players (0 = unchanged)
        max_players: Option<u16>,
    },

    /// Set player PVP zone status (for future L1 sync)
    ///
    /// Accounts:
    /// 0. `[writable]` World player account
    /// 1. `[signer]` Player authority
    SetPvpZone {
        in_pvp_zone: bool,
    },

    /// Move player with 3D input (camera-relative movement + physics)
    ///
    /// Accounts:
    /// 0. `[]` World config account
    /// 1. `[writable]` World player account
    /// 2. `[signer]` Player authority
    MovePlayer3D {
        /// 3D movement input (camera-relative with jump)
        input: MovementInput3D,
    },
}
