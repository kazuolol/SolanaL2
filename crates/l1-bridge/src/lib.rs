//! L1 Bridge - Settlement and L1 state reading
//!
//! Handles communication with Solana L1:
//! - Fetching delegated accounts from L1
//! - Committing WorldPlayer state back to L1
//! - Reading L1 state for future integration

pub mod committer;
pub mod delegator;
pub mod l1_reader;

pub use committer::StateCommitter;
pub use delegator::AccountDelegator;
pub use l1_reader::L1Reader;
