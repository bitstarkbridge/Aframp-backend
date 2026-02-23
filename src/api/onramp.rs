//! Onramp API endpoints
//!
//! Handles onramp transaction status tracking and monitoring

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::cache::cache::Cache;
use crate::cache::RedisCache;
use crate::chains::stellar::client::StellarClient;
use crate::database::transaction_repository::TransactionRepository;
use crate::error::{AppError, AppErrorKind, DomainError, ErrorCode};
use crate::payments::factory::PaymentProviderFactory;
use crate::payments::provider::PaymentProvider;
use crate::payments::types::{PaymentState, StatusRequest};

/// Transaction stage for UI rendering
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStage {
    AwaitingPayment,
    SendingCngn,
    Done,
    Failed,
    Refunded,
}

/// Provider status information
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub confirmed: bool,
    pub reference: String,
    pub checked_at: DateTime<Utc>,
}

/// Blockchain status information
#[derive(Debug, Clone, Serialize)]
pub struct BlockchainStatus {
    pub stellar_tx_hash: String,
    pub confirmations: u32,
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_url: Option<String>,
    pub checked_at: DateTime<Utc>,
}

/// Timeline entry for transaction history
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub status: String,
    pub timestamp: DateTime<Utc>,
    pub note: String,
}

/// Transaction detail for response
#[derive(Debug, Clone, Serialize)]
pub struct TransactionDetail {
    pub r#type: String,
    pub amount_ngn: i64,
    pub amount_cngn: i64,
    pub fees: FeeDetail,
    pub provider: String,
    pub wallet_address: String,
    pub chain: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

/// Fee detail structure
#[derive(Debug, Clone, Serialize)]
pub struct FeeDetail {
    pub platform_fee_ngn: i64,
    pub provider_fee_ngn: i64,
    pub total_fee_ngn: i64,
}

/// Onramp status response
#[derive(Debug, Clone, Serialize)]
pub struct OnrampStatusResponse {
    pub tx_id: String,
    pub status: String,
    pub stage: TransactionStage,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    pub transaction: TransactionDetail,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_status: Option<ProviderStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blockchain: Option<BlockchainStatus>,
    pub timeline: Vec<TimelineEntry>,
}

/// Error response structure
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

/// Service dependencies for onramp status endpoint
#[derive(Clone)]
pub struct OnrampStatusService {
    pub transaction_repo: Arc<TransactionRepository>,
    pub cache: Arc<RedisCache>,
    pub stellar_client: Arc<StellarClient>,
    pub payment_factory: Arc<PaymentProviderFactory>,
}

impl OnrampStatusService {
    pub fn new(
        transaction_repo: Arc<TransactionRepository>,
        cache: Arc<RedisCache>,
        stellar_client: Arc<StellarClient>,
        payment_factory: Arc<PaymentProviderFactory>,
    ) -> Self {
        Self {
            transaction_repo,
            cache,
            stellar_client,
            payment_factory,
        }
    }

    /// Get cache TTL based on transaction status
    fn get_cache_ttl(&self, status: &str) -> u64 {
        match status {
            "pending" => 5,
            "processing" => 10,
            "completed" | "failed" | "refunded" => 300,
            _ => 60,
        }
    }

    /// Build cache key for transaction status
    fn cache_key(&self, tx_id: &str) -> String {
        format!("api:onramp:status:{}", tx_id)
    }

    /// Check payment provider status for pending transactions
    async fn check_provider_status(
        &self,
        provider_name: &str,
        provider_reference: &str,
    ) -> Option<ProviderStatus> {
        debug!(
            "Checking provider status: provider={}, reference={}",
            provider_name, provider_reference
        );

        let provider = match self.payment_factory.get_provider(provider_name) {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to get payment provider {}: {}", provider_name, e);
                return None;
            }
        };

        let status_request = StatusRequest {
            transaction_reference: String::new(),
            provider_reference: Some(provider_reference.to_string()),
        };

        match provider.get_payment_status(status_request).await {
            Ok(response) => {
                let confirmed = matches!(response.status, PaymentState::Success);
                Some(ProviderStatus {
                    confirmed,
                    reference: provider_reference.to_string(),
                    checked_at: Utc::now(),
                })
            }
            Err(e) => {
                warn!(
                    "Failed to check provider status for {}: {}",
                    provider_reference, e
                );
                None
            }
        }
    }

    /// Check Stellar blockchain confirmation for processing transactions
    async fn check_blockchain_status(&self, tx_hash: &str) -> Option<BlockchainStatus> {
        debug!("Checking blockchain status for tx_hash={}", tx_hash);

        if tx_hash == "pending" || tx_hash.is_empty() {
            return Some(BlockchainStatus {
                stellar_tx_hash: "pending".to_string(),
                confirmations: 0,
                confirmed: false,
                explorer_url: None,
                checked_at: Utc::now(),
            });
        }

        match self.stellar_client.get_transaction_by_hash(tx_hash).await {
            Ok(tx_record) => {
                let network = self.stellar_client.network();
                let explorer_url = if tx_record.successful {
                    Some(format!(
                        "https://stellar.expert/explorer/{}/tx/{}",
                        match network {
                            crate::chains::stellar::config::StellarNetwork::Testnet => "testnet",
                            crate::chains::stellar::config::StellarNetwork::Mainnet => "public",
                        },
                        tx_hash
                    ))
                } else {
                    None
                };

                Some(BlockchainStatus {
                    stellar_tx_hash: tx_hash.to_string(),
                    confirmations: if tx_record.successful { 1 } else { 0 },
                    confirmed: tx_record.successful,
                    explorer_url,
                    checked_at: Utc::now(),
                })
            }
            Err(e) => {
                warn!("Failed to check blockchain status for {}: {}", tx_hash, e);
                None
            }
        }
    }

    /// Build timeline from transaction metadata
    fn build_timeline(
        &self,
        status: &str,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        metadata: &serde_json::Value,
    ) -> Vec<TimelineEntry> {
        let mut timeline = vec![TimelineEntry {
            status: "pending".to_string(),
            timestamp: created_at,
            note: "Transaction initiated".to_string(),
        }];

        // Add processing entry if status is processing or beyond
        if status == "processing" || status == "completed" {
            timeline.push(TimelineEntry {
                status: "processing".to_string(),
                timestamp: updated_at,
                note: "Payment confirmed".to_string(),
            });
        }

        // Add final status entry
        if status == "completed" {
            timeline.push(TimelineEntry {
                status: "completed".to_string(),
                timestamp: updated_at,
                note: "cNGN sent on Stellar".to_string(),
            });
        } else if status == "failed" {
            timeline.push(TimelineEntry {
                status: "failed".to_string(),
                timestamp: updated_at,
                note: metadata
                    .get("failure_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Transaction failed")
                    .to_string(),
            });
        } else if status == "refunded" {
            timeline.push(TimelineEntry {
                status: "refunded".to_string(),
                timestamp: updated_at,
                note: "Refund processed".to_string(),
            });
        }

        timeline
    }

    /// Determine transaction stage from status
    fn get_stage(&self, status: &str) -> TransactionStage {
        match status {
            "pending" => TransactionStage::AwaitingPayment,
            "processing" => TransactionStage::SendingCngn,
            "completed" => TransactionStage::Done,
            "failed" => TransactionStage::Failed,
            "refunded" => TransactionStage::Refunded,
            _ => TransactionStage::AwaitingPayment,
        }
    }

    /// Get user-friendly message for status
    fn get_message(&self, status: &str, provider: &str, amount_cngn: i64) -> String {
        match status {
            "pending" => format!("Waiting for your payment to be confirmed by {}.", provider),
            "processing" => format!(
                "Payment confirmed. Sending {} cNGN to your wallet.",
                amount_cngn
            ),
            "completed" => format!("{} cNGN has been sent to your wallet successfully.", amount_cngn),
            "failed" => "Transaction failed. If any payment was taken, a refund will be initiated automatically.".to_string(),
            "refunded" => "Transaction was refunded successfully.".to_string(),
            _ => "Transaction status unknown.".to_string(),
        }
    }

    /// Get transaction status with enrichment
    pub async fn get_status(&self, tx_id: &str) -> Result<OnrampStatusResponse, AppError> {
        // 1. Check cache first
        let cache_key = self.cache_key(tx_id);
        if let Ok(Some(cached)) = self.cache.get::<OnrampStatusResponse>(&cache_key).await {
            debug!("Cache hit for tx_id={}", tx_id);
            return Ok(cached);
        }

        // 2. Fetch transaction from database
        let transaction = self
            .transaction_repo
            .find_by_id(tx_id)
            .await
            .map_err(|e| {
                error!("Database error fetching transaction {}: {}", tx_id, e);
                AppError::new(AppErrorKind::Infrastructure(
                    crate::error::InfrastructureError::Database {
                        message: e.to_string(),
                        is_retryable: true,
                    },
                ))
            })?
            .ok_or_else(|| {
                debug!("Transaction not found: {}", tx_id);
                AppError::new(AppErrorKind::Domain(DomainError::TransactionNotFound {
                    transaction_id: tx_id.to_string(),
                }))
            })?;

        let status = transaction.status.as_str();
        let provider = transaction
            .payment_provider
            .as_deref()
            .unwrap_or("unknown");

        // 3. Enrich with provider status for pending transactions
        let provider_status = if status == "pending" {
            if let Some(ref provider_ref) = transaction.payment_reference {
                self.check_provider_status(provider, provider_ref).await
            } else {
                None
            }
        } else {
            transaction.payment_reference.as_ref().map(|reference| ProviderStatus {
                confirmed: status != "pending",
                reference: reference.clone(),
                checked_at: transaction.updated_at,
            })
        };

        // 4. Enrich with blockchain status for processing transactions
        let blockchain = if status == "processing" || status == "completed" {
            if let Some(ref tx_hash) = transaction.blockchain_tx_hash {
                self.check_blockchain_status(tx_hash).await
            } else {
                Some(BlockchainStatus {
                    stellar_tx_hash: "pending".to_string(),
                    confirmations: 0,
                    confirmed: false,
                    explorer_url: None,
                    checked_at: Utc::now(),
                })
            }
        } else {
            None
        };

        // 5. Extract fees from metadata
        let fees = transaction
            .metadata
            .get("fees")
            .and_then(|f| {
                Some(FeeDetail {
                    platform_fee_ngn: f.get("platform_fee_ngn")?.as_i64()?,
                    provider_fee_ngn: f.get("provider_fee_ngn")?.as_i64()?,
                    total_fee_ngn: f.get("total_fee_ngn")?.as_i64()?,
                })
            })
            .unwrap_or(FeeDetail {
                platform_fee_ngn: 0,
                provider_fee_ngn: 0,
                total_fee_ngn: 0,
            });

        // 6. Build response
        let amount_ngn = transaction.from_amount.to_string().parse::<f64>().unwrap_or(0.0) as i64;
        let amount_cngn = transaction.to_amount.to_string().parse::<f64>().unwrap_or(0.0) as i64;

        let response = OnrampStatusResponse {
            tx_id: transaction.transaction_id.to_string(),
            status: status.to_string(),
            stage: self.get_stage(status),
            message: self.get_message(status, provider, amount_cngn),
            failure_reason: transaction.error_message.clone(),
            transaction: TransactionDetail {
                r#type: transaction.r#type.clone(),
                amount_ngn,
                amount_cngn,
                fees,
                provider: provider.to_string(),
                wallet_address: transaction.wallet_address.clone(),
                chain: "stellar".to_string(),
                created_at: transaction.created_at,
                updated_at: transaction.updated_at,
                completed_at: if status == "completed" {
                    Some(transaction.updated_at)
                } else {
                    None
                },
            },
            provider_status,
            blockchain,
            timeline: self.build_timeline(
                status,
                transaction.created_at,
                transaction.updated_at,
                &transaction.metadata,
            ),
        };

        // 7. Cache response with appropriate TTL
        let ttl = self.get_cache_ttl(status);
        if let Err(e) = self.cache.set(&cache_key, &response, ttl).await {
            warn!("Failed to cache status for {}: {}", tx_id, e);
        }

        Ok(response)
    }
}

/// GET /api/onramp/status/:tx_id handler
pub async fn get_onramp_status(
    State(service): State<Arc<OnrampStatusService>>,
    Path(tx_id): Path<String>,
) -> Result<Json<OnrampStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    info!("GET /api/onramp/status/{}", tx_id);

    match service.get_status(&tx_id).await {
        Ok(response) => Ok(Json(response)),
        Err(err) => {
            let status_code = StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let error_response = ErrorResponse {
                error: ErrorDetail {
                    code: err.error_code(),
                    message: err.user_message(),
                    tx_id: Some(tx_id),
                    retry_after: if err.is_retryable() { Some(10) } else { None },
                },
            };
            Err((status_code, Json(error_response)))
        }
    }
}
