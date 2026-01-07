//! World Program Errors

use solana_program::program_error::ProgramError;
use thiserror::Error;

/// World program errors
#[derive(Error, Debug, Clone, Copy)]
pub enum WorldError {
    #[error("World is full")]
    WorldFull,

    #[error("Player not found")]
    PlayerNotFound,

    #[error("Player already exists")]
    PlayerAlreadyExists,

    #[error("Invalid direction")]
    InvalidDirection,

    #[error("Player is dead")]
    PlayerDead,

    #[error("Cannot attack self")]
    CannotAttackSelf,

    #[error("Target out of range")]
    TargetOutOfRange,

    #[error("Invalid authority")]
    InvalidAuthority,

    #[error("Invalid world")]
    InvalidWorld,

    #[error("Invalid account owner")]
    InvalidAccountOwner,

    #[error("Account not initialized")]
    AccountNotInitialized,

    #[error("Account already initialized")]
    AccountAlreadyInitialized,

    #[error("Arithmetic overflow")]
    ArithmeticOverflow,

    #[error("Invalid instruction data")]
    InvalidInstructionData,
}

impl From<WorldError> for ProgramError {
    fn from(e: WorldError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
