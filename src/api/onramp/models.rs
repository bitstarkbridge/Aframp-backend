use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::BigDecimal;
use uuid::Uuid;

/// Chain type for onramp transactions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Stellar,
}

impl Default for Chain {
    fn default() -> Self {
        Chain::Stellar
    }
}

/// Quote status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QuoteStatus {
    Pending,
    Consumed,
}

/// Request to create an onramp quote
#[derive(Debug, Deserialize)]
pub struct OnrampQuoteRequest {
    pub amount_ngn: BigDecimal,
    pub wallet_address: String,
    pub provider: String,
    #[serde(default)]
    pub chain: Chain,
}

/// Response containing the quote details
#[derive(Debug, Serialize)]
pub struct OnrampQuoteResponse {
    pub quote_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub expires_in_seconds: i64,
    pub input: QuoteInput,
    pub fees: QuoteFeeBreakdown,
    pub output: QuoteOutput,
    pub trustline_required: bool,
    pub liquidity_available: bool,
}

/// Input details of the quote
#[derive(Debug, Serialize)]
pub struct QuoteInput {
    pub amount_ngn: String,
    pub provider: String,
    pub chain: Chain,
    pub wallet_address: String,
}

/// Fee breakdown for the quote
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QuoteFeeBreakdown {
    pub platform_fee_ngn: String,
    pub provider_fee_ngn: String,
    pub total_fee_ngn: String,
    pub platform_fee_pct: String,
    pub provider_fee_pct: String,
}

/// Output details of the quote
#[derive(Debug, Serialize)]
pub struct QuoteOutput {
    pub amount_ngn_after_fees: String,
    pub rate: String,
    pub rate_source: String,
    pub rate_snapshot_at: DateTime<Utc>,
    pub amount_cngn: String,
    pub chain: Chain,
}

/// Stored quote in Redis
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredQuote {
    pub quote_id: Uuid,
    pub status: QuoteStatus,
    pub wallet_address: String,
    pub amount_ngn: String,
    pub amount_cngn: String,
    pub rate_snapshot: String,
    pub rate_snapshot_at: DateTime<Utc>,
    pub fees: QuoteFeeBreakdown,
    pub provider: String,
    pub chain: Chain,
    pub trustline_required: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
