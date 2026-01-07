//! L2 Consensus - Leader-based execution with validator broadcast
//!
//! Architecture:
//! - One leader executes transactions at 30ms tick rate
//! - Leader broadcasts StateChanges to connected validators
//! - Validators verify and apply changes, can challenge fraud
//! - Periodic checkpoints for L1 settlement

pub mod types;
pub mod leader;
pub mod validator;
pub mod broadcast;

pub use types::*;
pub use leader::{LeaderNode, LeaderNodeBuilder};
pub use validator::{ValidatorNode, ValidatorNodeBuilder};
pub use broadcast::{BroadcastServer, BroadcastClient};
