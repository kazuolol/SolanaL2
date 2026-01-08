//! L2 Runtime - Core SVM execution engine
//!
//! This crate provides the core runtime for the L2 gaming chain:
//! - Transaction processing via solana-svm
//! - In-memory account storage with optional disk persistence
//! - 30Hz block production loop

pub mod account_store;
pub mod block_producer;
pub mod callback;
pub mod persistence;
pub mod processor;

pub use account_store::AccountStore;
pub use block_producer::{BlockProducer, BlockProducerConfig, BlockUpdate, TransactionSender};
pub use callback::L2AccountLoader;
pub use persistence::{AccountStorePersistence, ChainMetadata, PersistentStore};
pub use processor::{L2Processor, TransactionResult};

/// Block time in milliseconds (30Hz = ~33.3ms)
pub const BLOCK_TIME_MS: u64 = 33;

/// Ticks per second
pub const TICKS_PER_SECOND: u64 = 30;

/// Maximum transactions per block
pub const MAX_TXS_PER_BLOCK: usize = 64;
