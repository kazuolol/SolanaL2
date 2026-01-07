//! World Program - L2 Game Logic
//!
//! Handles player positioning, movement, combat, and healing.
//! This runs on the L2 SVM chain, NOT on Solana L1.
//!
//! Account Structure:
//! - WorldConfig: Global world configuration
//! - WorldPlayer: Per-player state (position, health, etc.)

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

pub mod state;
pub mod instruction;
pub mod processor;
pub mod error;
pub mod builtin;

pub use state::{WorldConfig, WorldPlayer, MovementInput, MovementInput3D, WeaponStats};
pub use instruction::WorldInstruction;
pub use error::WorldError;

// World Program ID - unique identifier for the L2 game world program
// Note: base58 excludes: 0, I, O, l (lowercase L)
solana_program::declare_id!("Wor1dProgram1111111111111111111111111111111");

// Entry point
#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

/// Program entrypoint
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    processor::process(program_id, accounts, instruction_data)
}

/// Constants
pub mod constants {
    // Health/Combat
    /// Default player health
    pub const DEFAULT_HEALTH: u16 = 100;
    /// Default max health
    pub const DEFAULT_MAX_HEALTH: u16 = 100;
    /// Default damage (when L1 inventory not available)
    pub const DEFAULT_DAMAGE: u16 = 10;
    /// Default heal amount
    pub const DEFAULT_HEAL: u16 = 20;

    // Movement speeds
    /// Sprint speed (units per tick)
    pub const SPRINT_SPEED: i16 = 500;
    /// Normal walking speed
    pub const NORMAL_SPEED: i16 = 250;
    /// Acceleration per tick
    pub const ACCELERATION: i16 = 100;
    /// Friction/deceleration per tick when no input
    pub const FRICTION: i16 = 50;

    // Vertical physics (Y axis)
    /// Gravity per tick (negative = down)
    pub const GRAVITY: i16 = -30;
    /// Initial jump velocity
    pub const JUMP_VELOCITY: i16 = 400;
    /// Terminal falling velocity
    pub const TERMINAL_VELOCITY: i16 = -800;
    /// Ground level (Y = 0)
    pub const GROUND_LEVEL: i32 = 0;
    /// Maximum height for jumping
    pub const MAX_HEIGHT: i32 = 50_000; // 50 world units

    // Scale
    /// Fixed point scale (1000 = 1.0)
    pub const FIXED_POINT_SCALE: i32 = 1000;

    // PDA seeds
    /// World seed
    pub const WORLD_SEED: &[u8] = b"world";
    /// World player seed
    pub const WORLD_PLAYER_SEED: &[u8] = b"world_player";

    // Legacy (kept for compatibility)
    pub const MAX_SPEED: i16 = SPRINT_SPEED;
}
