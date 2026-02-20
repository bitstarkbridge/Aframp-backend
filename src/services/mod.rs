//! Services module for business logic and integrations

#[cfg(feature = "database")]
pub mod cngn_trustline;
#[cfg(feature = "database")]
pub mod cngn_payment_builder;
#[cfg(feature = "database")]
pub mod conversion_audit;
#[cfg(feature = "database")]
pub mod fee_structure;
#[cfg(feature = "database")]
pub mod trustline_operation;

// Re-export blockchain traits for convenience
#[cfg(feature = "database")]
pub use crate::chains::traits::{
    AggregatedBalance, BlockchainError, BlockchainResult, BlockchainService, ChainHealthStatus,
    ChainType, FeeEstimate, MultiChainBalanceAggregator, TotalBalance, TransactionBuilder,
    TransactionHandler, TransactionResult, TxParams,
};
