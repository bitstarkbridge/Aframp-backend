//! Onramp Quote Service
//!
//! Generates time-bound NGN → cNGN conversion quotes for the onramp flow.
//! Handles rate fetch, fee application, liquidity check, and DB persistence.

use crate::chains::stellar::client::StellarClient;
use crate::chains::stellar::types::extract_cngn_balance;
use crate::database::onramp_quote_repository::OnrampQuoteRepository;
use crate::error::{AppError, AppErrorKind, DomainError, ValidationError};
use crate::services::exchange_rate::{ConversionDirection, ConversionRequest, ExchangeRateService};
use crate::services::fee_structure::{FeeCalculationInput, FeeStructureService};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

/// Minimum onramp amount in NGN
const MIN_AMOUNT_NGN: i64 = 100;
/// Maximum onramp amount in NGN (configurable via env)
const DEFAULT_MAX_AMOUNT_NGN: i64 = 50_000_000;
/// Quote TTL in seconds (3 minutes, within 2–5 min range)
const QUOTE_TTL_SECS: i64 = 180;

/// Request body: amount_ngn only
#[derive(Debug, Clone, Deserialize)]
pub struct OnrampQuoteRequest {
    pub amount_ngn: i64,
}

/// Response format
#[derive(Debug, Clone, Serialize)]
pub struct OnrampQuoteResponse {
    pub quote_id: String,
    pub amount_ngn: i64,
    pub exchange_rate: f64,
    pub gross_cngn: f64,
    pub fee_cngn: f64,
    pub net_cngn: f64,
    pub expires_at: String,
}

pub struct OnrampQuoteService {
    exchange_rate_service: Arc<ExchangeRateService>,
    fee_service: Arc<FeeStructureService>,
    quote_repo: OnrampQuoteRepository,
    stellar_client: Option<StellarClient>,
    cngn_issuer: String,
    max_amount_ngn: i64,
}

impl OnrampQuoteService {
    pub fn new(
        exchange_rate_service: Arc<ExchangeRateService>,
        fee_service: Arc<FeeStructureService>,
        quote_repo: OnrampQuoteRepository,
        stellar_client: Option<StellarClient>,
        cngn_issuer: String,
    ) -> Self {
        let max_amount_ngn = std::env::var("ONRAMP_MAX_AMOUNT_NGN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_AMOUNT_NGN);

        Self {
            exchange_rate_service,
            fee_service,
            quote_repo,
            stellar_client,
            cngn_issuer,
            max_amount_ngn,
        }
    }

    /// Create a time-bound onramp quote
    pub async fn create_quote(
        &self,
        request: OnrampQuoteRequest,
    ) -> Result<OnrampQuoteResponse, AppError> {
        // 1. Validation
        if request.amount_ngn <= 0 {
            return Err(AppError::new(AppErrorKind::Validation(
                ValidationError::InvalidAmount {
                    amount: request.amount_ngn.to_string(),
                    reason: "Amount must be positive".to_string(),
                },
            )));
        }
        if request.amount_ngn < MIN_AMOUNT_NGN {
            return Err(AppError::new(AppErrorKind::Validation(
                ValidationError::OutOfRange {
                    field: "amount_ngn".to_string(),
                    min: Some(MIN_AMOUNT_NGN.to_string()),
                    max: None,
                },
            )));
        }
        if request.amount_ngn > self.max_amount_ngn {
            return Err(AppError::new(AppErrorKind::Validation(
                ValidationError::OutOfRange {
                    field: "amount_ngn".to_string(),
                    min: None,
                    max: Some(self.max_amount_ngn.to_string()),
                },
            )));
        }

        let amount_bd = BigDecimal::from(request.amount_ngn);

        // 2. Fetch rate and calculate conversion
        let conversion = self
            .exchange_rate_service
            .calculate_conversion(ConversionRequest {
                from_currency: "NGN".to_string(),
                to_currency: "cNGN".to_string(),
                amount: amount_bd.clone(),
                direction: ConversionDirection::Buy,
            })
            .await
            .map_err(|e| {
                AppError::new(AppErrorKind::External(crate::error::ExternalError::Blockchain {
                    message: format!("Rate fetch failed: {}", e),
                    is_retryable: true,
                }))
            })?;

        let rate = BigDecimal::from_str(&conversion.base_rate).unwrap_or_else(|_| BigDecimal::from(1));
        let gross_bd = BigDecimal::from_str(&conversion.gross_amount).unwrap_or_else(|_| amount_bd.clone());
        let fee_bd = BigDecimal::from_str(&conversion.fees.total_fees).unwrap_or_else(|_| BigDecimal::from(0));
        let net_bd = &gross_bd - &fee_bd;

        // 3. Liquidity check
        self.check_liquidity(&net_bd).await?;

        // 4. Persist quote in DB
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(QUOTE_TTL_SECS);

        let created = self
            .quote_repo
            .create(
                &amount_bd,
                &rate,
                &gross_bd,
                &fee_bd,
                &net_bd,
                expires_at,
            )
            .await
            .map_err(|e| {
                AppError::new(AppErrorKind::Infrastructure(
                    crate::error::InfrastructureError::Database {
                        message: format!("Failed to persist quote: {}", e),
                        is_retryable: true,
                    },
                ))
            })?;

        let quote_id = created.quote_id;
        debug!(quote_id = %quote_id, "Quote persisted to DB");

        let gross_f64: f64 = gross_bd.to_string().parse().unwrap_or(0.0);
        let fee_f64: f64 = fee_bd.to_string().parse().unwrap_or(0.0);
        let net_f64: f64 = net_bd.to_string().parse().unwrap_or(0.0);
        let rate_f64: f64 = rate.to_string().parse().unwrap_or(1.0);

        Ok(OnrampQuoteResponse {
            quote_id: quote_id.to_string(),
            amount_ngn: request.amount_ngn,
            exchange_rate: rate_f64,
            gross_cngn: gross_f64,
            fee_cngn: fee_f64,
            net_cngn: net_f64,
            expires_at: expires_at.to_rfc3339(),
        })
    }

    async fn check_liquidity(&self, amount_cngn: &BigDecimal) -> Result<(), AppError> {
        let Some(ref client) = self.stellar_client else {
            return Ok(()); // Skip if no Stellar client
        };

        let distribution = std::env::var("CNGN_DISTRIBUTION_ACCOUNT")
            .or_else(|_| std::env::var("CNGN_ISSUER_ADDRESS"))
            .unwrap_or_else(|_| self.cngn_issuer.clone());

        let account = client.get_account(&distribution).await.map_err(|e| {
            info!("Liquidity check skipped: {}", e);
            AppError::new(AppErrorKind::External(crate::error::ExternalError::Blockchain {
                message: e.to_string(),
                is_retryable: true,
            }))
        })?;

        let available = extract_cngn_balance(&account.balances, Some(&self.cngn_issuer));
        let available_bd: BigDecimal = available
            .and_then(|s| BigDecimal::from_str(&s).ok())
            .unwrap_or_else(|| BigDecimal::from(0));

        if available_bd < *amount_cngn {
            return Err(AppError::new(AppErrorKind::Domain(
                DomainError::InsufficientLiquidity {
                    amount: amount_cngn.to_string(),
                },
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_min_max_constants() {
        assert!(MIN_AMOUNT_NGN > 0);
        assert!(QUOTE_TTL_SECS >= 120 && QUOTE_TTL_SECS <= 300);
    }
}
